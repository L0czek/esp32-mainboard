use defmt::{error, info};
use embassy_futures::select::{self, Either3, Either4};
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
use mainboard::{board::{POWER_STATE, ADC_STATE}, power::PowerControllerStats};
use crate::simple_output::{set_state, watch_output};
use alloc::string::String;

use crate::wifi::WifiResources;
use crate::{
    html::INDEX_HTML,
    simple_output::OutputID,
};
use bq24296m;
use mainboard::board::POWER_CONTROL;
use mainboard::tasks::{PowerRequest, PowerResponse};

// Define the pool size for web tasks
const WEB_TASK_POOL_SIZE: usize = 8;

#[derive(Serialize)]
struct PinStatesResponse<'a> {
    pin_number: u8,
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
}

#[derive(Serialize)]
pub struct AdcVoltageResponse {
    pub battery_voltage: u32,
    pub boost_voltage: u32,
    pub a0: u32,
    pub a1: u32,
    pub a2: u32,
    pub a3: u32,
    pub a4: u32,
}

// App properties for the web server
struct AppProps;

impl AppBuilder for AppProps {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        Router::new()
            .route("/", routing::get_service(File::html(INDEX_HTML)))
            .route(
                "/ws",
                get(|upgrade: picoserve::response::WebSocketUpgrade| {
                    upgrade.on_upgrade(WebsocketHandler)
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
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum WebSocketCommand {
    #[serde(rename = "digital")]
    Digital { id: u8, value: u8 },
    #[serde(rename = "power")]
    Power { action: String, value: bool },
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
}

struct WebsocketHandler;

impl ws::WebSocketCallback for WebsocketHandler {
    async fn run<R: embedded_io_async::Read, W: embedded_io_async::Write<Error = R::Error>>(
        self,
        mut rx: ws::SocketRx<R>,
        mut tx: ws::SocketTx<W>,
    ) -> Result<(), W::Error> {
        let mut buffer = [0; 1024];

        let Some(mut power_state_receiver) = POWER_STATE.receiver() else {
            error!("Failed to get power state receiver");
            let _ = tx.close(Some((1011, "Failed to get power state receiver"))).await;
            return Ok(());
        };
        let Some(mut out1_receiver) = watch_output(OutputID::OutputD0) else {
            error!("Failed to watch output 1");
            let _ = tx.close(Some((1011, "Failed to watch output 1"))).await;
            return Ok(());
        };
        let Some(mut out2_receiver) = watch_output(OutputID::OutputD1) else {
            error!("Failed to watch output 2");
            let _ = tx.close(Some((1011, "Failed to watch output 2"))).await;
            return Ok(());
        };
        let Some(mut out3_receiver) = watch_output(OutputID::OutputD2) else {
            error!("Failed to watch output 3");
            let _ = tx.close(Some((1011, "Failed to watch output 3"))).await;
            return Ok(());
        };
        let Some(mut out4_receiver) = watch_output(OutputID::OutputD3) else {
            error!("Failed to watch output 4");
            let _ = tx.close(Some((1011, "Failed to watch output 4"))).await;
            return Ok(());
        };
        let Some(mut out5_receiver) = watch_output(OutputID::OutputD4) else {
            error!("Failed to watch output 5");
            let _ = tx.close(Some((1011, "Failed to watch output 5"))).await;
            return Ok(());
        };
        let Some(mut adc_state_receiver) = ADC_STATE.receiver() else {
            error!("Failed to get ADC state receiver");
            let _ = tx.close(Some((1011, "Failed to get ADC state receiver"))).await;
            return Ok(());
        };

        let close_reason = loop {
            match select::select4(
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
            ).await {
                Either4::First(x) => match x {
                    Ok(ws::Message::Text(data)) => {
                        if let Ok(command) = serde_json::from_str::<WebSocketCommand>(data) {
                            match command {
                                WebSocketCommand::Digital { id, value } => {
                                    let _ = set_state(match id {
                                        0 => OutputID::OutputD0,
                                        1 => OutputID::OutputD1,
                                        2 => OutputID::OutputD2,
                                        3 => OutputID::OutputD3,
                                        4 => OutputID::OutputD4,
                                        _ => {
                                            error!("Invalid output ID: {}", id);
                                            continue;
                                        }
                                    }, value != 0).await;
                                }
                                WebSocketCommand::Power { action, value } => match action.as_str() {
                                    "boost" => {
                                        info!("Setting boost converter to: {}", if value { "enabled" } else { "disabled" });
                                        match POWER_CONTROL.transact(PowerRequest::EnableBoostConverter(value)).await {
                                            PowerResponse::Ok => info!("Boost converter set successfully"),
                                            PowerResponse::Err(_) => info!("Failed to set boost converter state"),
                                        };
                                    }
                                    _ => error!("Unknown power action"),
                                },
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
                Either4::Second(power_state) => {
                    let power_stats_response = format_power_stats_response(power_state);
                    tx.send_json(OutgoingMessage::PowerStats(power_stats_response)).await
                }
                Either4::Third(adc_state) => {
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
                Either4::Fourth(pin_select) => {
                    match pin_select {
                        Either3::First(out1_state) => {
                            let pin_state_response = PinStatesResponse {
                                pin_number: 0,
                                state: out1_state.to_str(),
                            };
                            tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                        }
                        Either3::Second(out2_state) => {
                            let pin_state_response = PinStatesResponse {
                                pin_number: 1,
                                state: out2_state.to_str(),
                            };
                            tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                        }
                        Either3::Third(inner_select) => {
                            match inner_select {
                                Either3::First(out3_state) => {
                                    let pin_state_response = PinStatesResponse {
                                        pin_number: 2,
                                        state: out3_state.to_str(),
                                    };
                                    tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                                }
                                Either3::Second(out4_state) => {
                                    let pin_state_response = PinStatesResponse {
                                        pin_number: 3,
                                        state: out4_state.to_str(),
                                    };
                                    tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                                }
                                Either3::Third(out5_state) => {
                                    let pin_state_response = PinStatesResponse {
                                        pin_number: 4,
                                        state: out5_state.to_str(),
                                    };
                                    tx.send_json(OutgoingMessage::PinState(pin_state_response)).await
                                }
                            }
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
pub async fn run_server(spawner: embassy_executor::Spawner, wifi_resources: &WifiResources) {
    let WifiResources {
        ap_stack,
        sta_stack,
    } = wifi_resources;

    // Create the router app
    let app = make_static!(AppRouter<AppProps>, AppProps.build_app());

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
