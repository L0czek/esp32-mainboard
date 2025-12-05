use defmt::{error, info};
use embassy_futures::select::{self, Either, Either3, Either4};
use embassy_time::Duration;
use serde::{Deserialize, Serialize};
use serde_json;
use picoserve::{
    make_static,
    response::{ws, File},
    routing::{self, get, PathRouter},
    AppBuilder, AppRouter, Router,
};
extern crate alloc;
use mainboard::tasks::{
    AdcHandle,
    DigitalIoHandle,
    PowerHandle,
    UartHandle,
    PowerResponse,
    DigitalPinID,
    PinMode,
};
use mainboard::power::PowerControllerStats;
use alloc::string::String;
use alloc::vec::Vec;

use crate::wifi::WifiResources;
use bq24296m;

// Define the pool size for web tasks
const WEB_TASK_POOL_SIZE: usize = 8;

#[derive(Serialize)]
struct PinStatesResponse<'a> {
    pin_number: u8,
    mode: &'a str,
    state: &'a str,
}

#[derive(Serialize)]
struct PowerStatsResponse<'a> {
    // Status register decoded values
    vbus_status: &'a str,
    charge_status: &'a str,
    dpm_active: bool,
    power_good: bool,
    thermal_regulation_active: bool,
    vsys_regulation_active: bool,
    // Fault register decoded values
    watchdog_fault: bool,
    otg_fault: bool,
    charge_fault_status: &'a str,
    battery_fault: bool,
    ntc_fault_status: &'a str,
    ntc_cold_fault: bool,
    ntc_hot_fault: bool,
    // Boost converter state
    boost_converter_enabled: bool,
    // Input status pins
    vbus_present: bool,
    vbus_flg: bool,
    dc_jack_present: bool,
    // Output control pins
    chr_en: bool,
    chr_otg: bool,
    chr_psel: bool,
    vbus_enable: bool,
}

#[derive(Serialize)]
pub struct AdcVoltageResponse {
    pub battery_voltage: u16,
    pub boost_voltage: u16,
    pub a0: u16,
    pub a1: u16,
    pub a2: u16,
    pub a3: u16,
    pub a4: u16,
}

#[derive(Serialize)]
pub struct AdcBufferResponse {
    pub sequence: u32,
    pub battery_voltage: alloc::vec::Vec<u16>,
    pub boost_voltage: alloc::vec::Vec<u16>,
    pub a0: alloc::vec::Vec<u16>,
    pub a1: alloc::vec::Vec<u16>,
    pub a2: alloc::vec::Vec<u16>,
    pub a3: alloc::vec::Vec<u16>,
    pub a4: alloc::vec::Vec<u16>,
}

// App properties for the web server
#[derive(Clone, Copy)]
struct AppProps {
    power: PowerHandle,
    digital: DigitalIoHandle,
    adc: AdcHandle,
    uart: UartHandle,
}

#[derive(Clone, Copy)]
struct WebsocketHandler {
    power: PowerHandle,
    digital: DigitalIoHandle,
    adc: AdcHandle,
    uart: UartHandle,
}

impl AppBuilder for AppProps {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        let handler = WebsocketHandler {
            power: self.power,
            digital: self.digital,
            adc: self.adc,
            uart: self.uart,
        };

        Router::new()
            .route("/", routing::get_service(File::html(include_str!("index.html"))))
            .route(
                "/ws",
                get(move |upgrade: picoserve::response::WebSocketUpgrade| {
                    upgrade.on_upgrade(handler)
                }),
            )
    }
}

fn format_power_stats_response<'a>(stats: PowerControllerStats) -> PowerStatsResponse<'a> {
    // Use helper functions to decode register values
    let status = &stats.charger_status;
    let faults = &stats.charger_faults;

    info!("Power stats: retrieved successfully");

    // Convert enum values to static string slices to avoid lifetime issues
    let vbus_status = match status.get_vbus_status() {
        bq24296m::VbusStatus::Unknown => "Unknown",
        bq24296m::VbusStatus::UsbHost => "USB Host",
        bq24296m::VbusStatus::AdapterPort => "Adapter Port",
        bq24296m::VbusStatus::Otg => "OTG",
    };

    let charge_status = match status.get_charge_status() {
        bq24296m::ChargeStatus::NotCharging => "Not Charging",
        bq24296m::ChargeStatus::PreCharge => "Pre-Charge",
        bq24296m::ChargeStatus::FastCharging => "Fast Charging",
        bq24296m::ChargeStatus::ChargeDone => "Charge Done",
    };

    let charge_fault_status = match faults.get_charge_fault_status() {
        bq24296m::ChargeFaultStatus::Normal => "Normal",
        bq24296m::ChargeFaultStatus::InputFault => "Input Fault",
        bq24296m::ChargeFaultStatus::ThermalShutdown => "Thermal Shutdown",
        bq24296m::ChargeFaultStatus::ChargeTimerExpired => "Charge Timer Expired",
    };

    let ntc_fault_status = match faults.get_ntc_fault_status() {
        bq24296m::NtcFaultStatus::Normal => "Normal",
        bq24296m::NtcFaultStatus::Cold => "Cold",
        bq24296m::NtcFaultStatus::Hot => "Hot",
        bq24296m::NtcFaultStatus::ColdAndHot => "Cold and Hot",
    };

    PowerStatsResponse {
        // Status register decoded values
        vbus_status,
        charge_status,
        dpm_active: status.is_dpm_active(),
        power_good: status.is_power_good(),
        thermal_regulation_active: status.is_thermal_regulation_active(),
        vsys_regulation_active: status.is_vsys_regulation_active(),
        // Fault register decoded values
        watchdog_fault: faults.is_watchdog_fault(),
        otg_fault: faults.is_otg_fault(),
        charge_fault_status,
        battery_fault: faults.is_battery_fault(),
        ntc_fault_status,
        ntc_cold_fault: faults.is_ntc_cold_fault(),
        ntc_hot_fault: faults.is_ntc_hot_fault(),
        // Boost converter state
        boost_converter_enabled: stats.boost_enabled,
        // Input status pins
        vbus_present: stats.expander_status.vbus_present(),
        vbus_flg: stats.expander_status.vbus_flg(),
        dc_jack_present: stats.expander_status.dc_jack_present(),
        // Output control pins
        chr_en: stats.expander_status.chr_en(),
        chr_otg: stats.expander_status.chr_otg(),
        chr_psel: stats.expander_status.chr_psel(),
        vbus_enable: stats.expander_status.vbus_enable(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum WebSocketCommand {
    #[serde(rename = "digital")]
    Digital { id: u8, value: u8 },
    #[serde(rename = "digital_mode")]
    DigitalMode { id: u8, mode: String },
    #[serde(rename = "power")]
    Power { action: String, value: bool },
    #[serde(rename = "i2c_scan")]
    I2cScan,
    #[serde(rename = "i2c_read")]
    I2cRead { address: u8, register: u8 },
    #[serde(rename = "i2c_write")]
    I2cWrite { address: u8, register: u8, value: u8 },
    #[serde(rename = "uart_send")]
    UartSend { bytes: Vec<u8> },
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum OutgoingMessage<'a> {
    #[serde(rename = "power_stats")]
    PowerStats(PowerStatsResponse<'a>),
    #[serde(rename = "pin_state")]
    PinState(PinStatesResponse<'a>),
    #[serde(rename = "adc_voltage")]
    AdcVoltage(AdcVoltageResponse),
    #[serde(rename = "adc_buffer")]
    AdcBuffer(AdcBufferResponse),
    #[serde(rename = "i2c_scan_result")]
    I2cScanResult { devices: alloc::vec::Vec<u8> },
    #[serde(rename = "i2c_read_result")]
    I2cReadResult { address: u8, register: u8, value: u8, success: bool },
    #[serde(rename = "i2c_write_result")]
    I2cWriteResult { address: u8, register: u8, success: bool },
    #[serde(rename = "uart_receive")]
    UartReceive { bytes: alloc::vec::Vec<u8> },
}

// I2C helper functions
async fn i2c_scan() -> Vec<u8> {
    use embedded_hal::i2c::I2c as I2cTrait;
    let mut i2c = mainboard::board::acquire_i2c_bus();
    let mut devices = Vec::new();
    
    // Scan I2C address range (0x03 to 0x77)
    for addr in 0x03..=0x77 {
        // Try to write empty data to detect device presence
        if i2c.write(addr, &[]).is_ok() {
            devices.push(addr);
        }
    }
    
    info!("I2C scan found {} devices", devices.len());
    devices
}

async fn i2c_read(address: u8, register: u8) -> Result<u8, ()> {
    use embedded_hal::i2c::I2c as I2cTrait;
    let mut i2c = mainboard::board::acquire_i2c_bus();
    let mut buffer = [0u8; 1];
    
    match i2c.write_read(address, &[register], &mut buffer) {
        Ok(_) => {
            info!("I2C read: addr=0x{:02X}, reg=0x{:02X}, value=0x{:02X}", address, register, buffer[0]);
            Ok(buffer[0])
        }
        Err(_) => {
            error!("I2C read failed: addr=0x{:02X}, reg=0x{:02X}", address, register);
            Err(())
        }
    }
}

async fn i2c_write(address: u8, register: u8, value: u8) -> Result<(), ()> {
    use embedded_hal::i2c::I2c as I2cTrait;
    let mut i2c = mainboard::board::acquire_i2c_bus();
    
    match i2c.write(address, &[register, value]) {
        Ok(_) => {
            info!("I2C write: addr=0x{:02X}, reg=0x{:02X}, value=0x{:02X}", address, register, value);
            Ok(())
        }
        Err(_) => {
            error!("I2C write failed: addr=0x{:02X}, reg=0x{:02X}, value=0x{:02X}", address, register, value);
            Err(())
        }
    }
}

impl ws::WebSocketCallback for WebsocketHandler {
    async fn run<R: embedded_io_async::Read, W: embedded_io_async::Write<Error = R::Error>>(
        self,
        mut rx: ws::SocketRx<R>,
        mut tx: ws::SocketTx<W>,
    ) -> Result<(), W::Error> {
        let mut buffer = [0; 1024];

        let Some(mut power_state_receiver) = self.power.state_receiver() else {
            error!("Failed to get power state receiver");
            let _ = tx.close(Some((1011, "Failed to get power state receiver"))).await;
            return Ok(());
        };
        let Some(mut out1_receiver) = self.digital.watch(DigitalPinID::D0) else {
            error!("Failed to watch output 1");
            let _ = tx.close(Some((1011, "Failed to watch output 1"))).await;
            return Ok(());
        };
        let Some(mut out2_receiver) = self.digital.watch(DigitalPinID::D1) else {
            error!("Failed to watch output 2");
            let _ = tx.close(Some((1011, "Failed to watch output 2"))).await;
            return Ok(());
        };
        let Some(mut out3_receiver) = self.digital.watch(DigitalPinID::D2) else {
            error!("Failed to watch output 3");
            let _ = tx.close(Some((1011, "Failed to watch output 3"))).await;
            return Ok(());
        };
        let Some(mut out4_receiver) = self.digital.watch(DigitalPinID::D3) else {
            error!("Failed to watch output 4");
            let _ = tx.close(Some((1011, "Failed to watch output 4"))).await;
            return Ok(());
        };
        let Some(mut out5_receiver) = self.digital.watch(DigitalPinID::D4) else {
            error!("Failed to watch output 5");
            let _ = tx.close(Some((1011, "Failed to watch output 5"))).await;
            return Ok(());
        };
        let Some(mut adc_state_receiver) = self.adc.state_receiver() else {
            error!("Failed to get ADC state receiver");
            let _ = tx.close(Some((1011, "Failed to get ADC state receiver"))).await;
            return Ok(());
        };
        let Some(mut adc_buffer_subscriber) = self.adc.buffer_subscriber() else {
            error!("Failed to get ADC buffer subscriber");
            let _ = tx.close(Some((1011, "Failed to get ADC buffer subscriber"))).await;
            return Ok(());
        };
        let Some(mut uart_rx_subscriber) = self.uart.subscribe() else {
            error!("Failed to get UART RX subscriber");
            let _ = tx.close(Some((1011, "Failed to get UART RX subscriber"))).await;
            return Ok(());
        };

        let close_reason = loop {
            match select::select(
                select::select4(
                    rx.next_message(&mut buffer),
                    power_state_receiver.changed(),
                    adc_state_receiver.changed(),
                    select::select3(
                        out1_receiver.changed(),
                        out2_receiver.changed(),
                        select::select3(
                            out3_receiver.changed(),
                            out4_receiver.changed(),
                            out5_receiver.changed()
                        )
                    )
                ),
                select::select(
                    adc_buffer_subscriber.next_message_pure(),
                    uart_rx_subscriber.next_message_pure()
                )
            ).await {
                Either::First(Either4::First(x)) => match x {
                    Ok(ws::Message::Text(data)) => {
                        if let Ok(command) = serde_json::from_str::<WebSocketCommand>(data) {
                            match command {
                                WebSocketCommand::Digital { id, value } => {
                                    let pin = match id {
                                        0 => DigitalPinID::D0,
                                        1 => DigitalPinID::D1,
                                        2 => DigitalPinID::D2,
                                        3 => DigitalPinID::D3,
                                        4 => DigitalPinID::D4,
                                        _ => {
                                            error!("Invalid output ID: {}", id);
                                            continue;
                                        }
                                    };

                                    self.digital.set(pin, value != 0).await;
                                }
                                WebSocketCommand::DigitalMode { id, mode } => {
                                    let pin = match id {
                                        0 => DigitalPinID::D0,
                                        1 => DigitalPinID::D1,
                                        2 => DigitalPinID::D2,
                                        3 => DigitalPinID::D3,
                                        4 => DigitalPinID::D4,
                                        _ => {
                                            error!("Invalid output ID: {}", id);
                                            continue;
                                        }
                                    };

                                    let pin_mode = match mode.as_str() {
                                        "OpenDrain" => PinMode::OpenDrain,
                                        "PushPull" => PinMode::PushPull,
                                        _ => {
                                            error!("Invalid pin mode: {}", mode.as_str());
                                            continue;
                                        }
                                    };

                                    info!("Setting pin {} to mode {}", id, mode.as_str());
                                    self.digital.set_mode(pin, pin_mode).await;
                                }
                                WebSocketCommand::Power { action, value } => match action.as_str() {
                                    "boost" => {
                                        info!("Setting boost converter to: {}", if value { "enabled" } else { "disabled" });
                                        match self.power.set_boost_converter(value).await {
                                            PowerResponse::Ok => info!("Boost converter set successfully"),
                                            PowerResponse::Err(_) => info!("Failed to set boost converter state"),
                                        };
                                    }
                                    _ => error!("Unknown power action"),
                                },
                                WebSocketCommand::I2cScan => {
                                    info!("Starting I2C scan");
                                    let devices = i2c_scan().await;
                                    let _ = tx.send_json(OutgoingMessage::I2cScanResult { devices }).await;
                                }
                                WebSocketCommand::I2cRead { address, register } => {
                                    info!("I2C read request: addr=0x{:02X}, reg=0x{:02X}", address, register);
                                    match i2c_read(address, register).await {
                                        Ok(value) => {
                                            let _ = tx.send_json(OutgoingMessage::I2cReadResult { 
                                                address, 
                                                register, 
                                                value, 
                                                success: true 
                                            }).await;
                                        }
                                        Err(_) => {
                                            let _ = tx.send_json(OutgoingMessage::I2cReadResult { 
                                                address, 
                                                register, 
                                                value: 0, 
                                                success: false 
                                            }).await;
                                        }
                                    }
                                }
                                WebSocketCommand::I2cWrite { address, register, value } => {
                                    info!("I2C write request: addr=0x{:02X}, reg=0x{:02X}, value=0x{:02X}", address, register, value);
                                    let success = i2c_write(address, register, value).await.is_ok();
                                    let _ = tx.send_json(OutgoingMessage::I2cWriteResult { 
                                        address, 
                                        register, 
                                        success 
                                    }).await;
                                }
                                WebSocketCommand::UartSend { bytes } => {
                                    info!("UART send bytes request: {} bytes", bytes.len());
                                    self.uart.send(&bytes).await;
                                }
                            }
                        }
                        continue
                    }
                    Ok(ws::Message::Binary(_)) => break Some((1003, "Binary messages not supported")),
                    Ok(ws::Message::Close(_)) => break None,
                    Ok(ws::Message::Ping(data)) => tx.send_pong(data).await,
                    Ok(ws::Message::Pong(_)) => continue,
                    Err(err) => {
                        let code = match err {
                            ws::ReadMessageError::Io(err) => return Err(err),
                            ws::ReadMessageError::ReadFrameError(_)
                            | ws::ReadMessageError::MessageStartsWithContinuation
                            | ws::ReadMessageError::UnexpectedMessageStart => 1002,
                            ws::ReadMessageError::ReservedOpcode(_) => 1003,
                            ws::ReadMessageError::TextIsNotUtf8 => 1007,
                        };
                        break Some((code, "Websocket Error"));
                    }
                },
                Either::First(Either4::Second(power_state)) => {
                    let power_stats_response = format_power_stats_response(power_state);
                    tx.send_json(OutgoingMessage::PowerStats(power_stats_response)).await
                }
                Either::First(Either4::Third(adc_state)) => {
                    let adc_response = AdcVoltageResponse {
                        battery_voltage: adc_state.battery_voltage,
                        boost_voltage: adc_state.boost_voltage,
                        a0: adc_state.a0,
                        a1: adc_state.a1,
                        a2: adc_state.a2,
                        a3: adc_state.a3,
                        a4: adc_state.a4,
                    };
                    tx.send_json(OutgoingMessage::AdcVoltage(adc_response)).await
                }
                Either::First(Either4::Fourth(pin_select)) => {
                    match pin_select {
                        Either3::First((mode, state)) => {
                            let pin_state_response = PinStatesResponse {
                                pin_number: 0,
                                mode: mode.to_str(),
                                state: state.to_str(),
                            };
                            tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                        }
                        Either3::Second((mode, state)) => {
                            let pin_state_response = PinStatesResponse {
                                pin_number: 1,
                                mode: mode.to_str(),
                                state: state.to_str(),
                            };
                            tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                        }
                        Either3::Third(inner_select) => {
                            match inner_select {
                                Either3::First((mode, state)) => {
                                    let pin_state_response = PinStatesResponse {
                                        pin_number: 2,
                                        mode: mode.to_str(),
                                        state: state.to_str(),
                                    };
                                    tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                                }
                                Either3::Second((mode, state)) => {
                                    let pin_state_response = PinStatesResponse {
                                        pin_number: 3,
                                        mode: mode.to_str(),
                                        state: state.to_str(),
                                    };
                                    tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                                }
                                Either3::Third((mode, state)) => {
                                    let pin_state_response = PinStatesResponse {
                                        pin_number: 4,
                                        mode: mode.to_str(),
                                        state: state.to_str(),
                                    };
                                    tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                                }
                            }
                        }
                    }
                }
                Either::Second(data_select) => {
                    match data_select {
                        Either::First(buffer_data) => {
                            let buffer_response = AdcBufferResponse {
                                sequence: buffer_data.sequence,
                                battery_voltage: buffer_data.battery_voltage.to_vec(),
                                boost_voltage: buffer_data.boost_voltage.to_vec(),
                                a0: buffer_data.a0.to_vec(),
                                a1: buffer_data.a1.to_vec(),
                                a2: buffer_data.a2.to_vec(),
                                a3: buffer_data.a3.to_vec(),
                                a4: buffer_data.a4.to_vec(),
                            };
                            tx.send_json(OutgoingMessage::AdcBuffer(buffer_response)).await
                        }
                        Either::Second(uart_data) => {
                            tx.send_json(OutgoingMessage::UartReceive { 
                                bytes: uart_data.bytes 
                            }).await
                        }
                    }
                }
            }?;
        };

        tx.close(close_reason).await
    }
}

/// Initialize and run the web server
///
/// This function sets up the picoserve server using the provided WiFi resources
/// and spawns tasks to handle web requests.
pub async fn run_server(
    spawner: embassy_executor::Spawner,
    wifi_resources: &WifiResources,
    power: PowerHandle,
    adc: AdcHandle,
    digital: DigitalIoHandle,
    uart: UartHandle,
) {
    let WifiResources {
        ap_stack,
        sta_stack,
    } = wifi_resources;

    // Create the router app
    let app = make_static!(
        AppRouter<AppProps>,
        AppProps {
            power,
            digital,
            adc,
            uart,
        }
        .build_app()
    );

    // Configure server timeouts
    let config = make_static!(
        picoserve::Config<Duration>,
        picoserve::Config::new(picoserve::Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            persistent_start_read_request: Some(Duration::from_secs(3)),
            read_request: Some(Duration::from_secs(3)),
            write: Some(Duration::from_secs(3)),
        })
        .keep_connection_alive()
    );

    // No need for buffer allocation here

    // Start web tasks for AP interface
    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.spawn(web_task(id, *ap_stack, app, config)).unwrap();
    }

    // Start web tasks for STA interface
    for id in 0..WEB_TASK_POOL_SIZE {
        spawner
            .spawn(web_task(id + WEB_TASK_POOL_SIZE, *sta_stack, app, config))
            .unwrap();
    }

    info!(
        "Web server started on both AP ({}:80) and STA interfaces ({}:80)",
        ap_stack.config_v4().map(|c| c.address),
        sta_stack.config_v4().map(|c| c.address)
    );
}

// Web task function that handles HTTP requests
#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE * 2)]
async fn web_task(
    id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<AppProps>,
    config: &'static picoserve::Config<Duration>,
) -> ! {
    let port = 80;

    // Allocate buffers inside the task
    let mut tcp_rx_buffer = [0; 1024];
    let mut tcp_tx_buffer = [0; 1024];
    let mut http_buffer = [0; 2048];

    picoserve::listen_and_serve(
        id,
        app,
        config,
        stack,
        port,
        &mut tcp_rx_buffer,
        &mut tcp_tx_buffer,
        &mut http_buffer,
    )
    .await
}
