#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![recursion_limit = "256"]

mod adc;
mod digital_io;
mod server;
mod uart;

use esp_hal::analog::adc::AdcConfig;
use mainboard::board::{acquire_i2c_bus, init_i2c_bus, Board};
use mainboard::create_board;
use mainboard::power::PowerControllerIO;
use mainboard::tasks::{
    spawn_ext_interrupt_task, spawn_power_controller, PowerResponse, PowerStateReceiver,
};
use mainboard::wifi::initialize_wifi_mixed;

use crate::adc::{spawn_adc_task, AdcHandle, VoltageMonitorCalibrationConfig};
use crate::digital_io::{spawn_digital_io, DigitalPinID};
use crate::server::ShutdownHandle;
use crate::uart::spawn_uart_tasks;
use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_hal::analog::adc::Attenuation;
use esp_hal::efuse::{AdcCalibUnit, Efuse};
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{clock::CpuClock, rtc_cntl::Rtc};
use panic_rtt_target as _;
use static_cell::StaticCell;

// StaticCell for WiFi controller
static ESP_RADIO_INIT: StaticCell<esp_radio::Controller<'static>> = StaticCell::new();
static SHUTDOWN_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    // Initialize RTT for logging
    rtt_target::rtt_init_defmt!();

    // Configure and initialize hardware
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // Initialize heap allocator
    // Not #[esp_hal::ram(reclaimed)] because its too small XD
    // We use all we can afford to hopefully have enough for serde XD
    esp_alloc::heap_allocator!(size: 65536 * 2);

    // Initialize esp_rtos
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    info!("Embassy initialized!");

    dump_adc_efuse_calibration();

    let board = create_board!(peripherals);

    init_i2c_bus(peripherals.I2C0, board.Sda, board.Scl).expect("Failed to initialize I2C bus");

    // Initialize RNG for WiFi
    let mut rng = esp_hal::rng::Rng::new();

    // Initialize esp-radio controller
    let radio_init =
        ESP_RADIO_INIT.init(esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller"));

    let power_config = Default::default();
    let power_io = PowerControllerIO {
        charger_i2c: acquire_i2c_bus(),
        pcf8574_i2c: acquire_i2c_bus(),
        boost_converter_enable: board.BoostEn,
    };
    let power = spawn_power_controller(&spawner, power_config, power_io);
    let power_receiver = power
        .state_receiver()
        .expect("Failed to get power state receiver");
    spawner
        .spawn(log_power_state_changes_task(power_receiver))
        .expect("Failed to spawn log_power_state_changes_task");

    let adc_config = AdcConfig::new();
    let calibration: VoltageMonitorCalibrationConfig = Default::default();
    let adc = spawn_adc_task(
        &spawner,
        peripherals.ADC1,
        adc_config,
        calibration,
        board.BatVol,
        board.BoostVol,
        board.A0,
        board.A1,
        board.A2,
        board.A3,
        board.A4,
    );
    spawner
        .spawn(log_voltage_changes_task(adc))
        .expect("Failed to spawn log_voltage_changes_task");

    spawn_ext_interrupt_task(&spawner, board.GlobalInt, power, None);

    // Initialize UART
    info!("Initializing UART...");
    let uart = esp_hal::uart::Uart::new(peripherals.UART0, esp_hal::uart::Config::default())
        .unwrap()
        .with_rx(board.U0Rx)
        .with_tx(board.U0Tx);

    // Convert to async
    let uart = uart.into_async();
    let (uart_rx, uart_tx) = uart.split();
    let uart_handle = spawn_uart_tasks(&spawner, uart_rx, uart_tx);
    info!("UART initialized!");

    // Initialize WiFi in mixed mode (AP + STA)
    info!("Initializing WiFi...");
    let wifi_resources =
        initialize_wifi_mixed(spawner, radio_init, peripherals.WIFI, &mut rng).await;
    info!("WiFi initialized!");

    // Initialize simple output
    let digital = spawn_digital_io(&spawner, board.D0, board.D1, board.D2, board.D3, board.D4);

    // Start the web server
    info!("Starting web server...");
    let shutdown_handle = ShutdownHandle::new(&SHUTDOWN_SIGNAL);
    server::run_server(
        spawner,
        &wifi_resources,
        power,
        adc,
        digital,
        uart_handle,
        shutdown_handle,
    )
    .await;
    info!("Web server started!");

    // Main loop
    loop {
        match select(
            Timer::after(Duration::from_secs(10)),
            SHUTDOWN_SIGNAL.wait(),
        )
        .await
        {
            Either::First(_) => {
                info!(
                    "Server running... AP IP: {:?}, STA IP: {:?}",
                    wifi_resources.ap_stack.config_v4().map(|c| c.address),
                    wifi_resources.sta_stack.config_v4().map(|c| c.address)
                );
            }
            Either::Second(_) => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    // Perform shutdown sequence
    info!("Executing shutdown sequence: disable boost, set charger to Charging, float GPIOs");
    match power.set_boost_converter(false).await {
        PowerResponse::Ok => info!("Boost converter disabled"),
        PowerResponse::Err(e) => info!("Failed to disable boost converter: {:?}", e),
    }

    let pins = [
        DigitalPinID::D0,
        DigitalPinID::D1,
        DigitalPinID::D2,
        DigitalPinID::D3,
        DigitalPinID::D4,
    ];

    for pin in pins {
        digital.set_mode(pin, digital_io::PinMode::OpenDrain).await;
        digital.set(pin, true).await;
    }

    match power.enter_shipping_mode().await {
        PowerResponse::Ok => info!("Charger set to shipping mode"),
        PowerResponse::Err(e) => info!("Failed to enter shipping mode: {:?}", e),
    }

    info!("Entering deep sleep (shutdown)");
    let mut rtc = Rtc::new(peripherals.LPWR);
    rtc.sleep_deep(&[]);
}

fn dump_adc_efuse_calibration() {
    let (blk_major, blk_minor) = Efuse::block_version();
    info!(
        "Efuse: chip v{}.{}, block v{}.{}, rtc_calib v{}",
        Efuse::major_chip_version(),
        Efuse::minor_chip_version(),
        blk_major,
        blk_minor,
        Efuse::rtc_calib_version(),
    );

    let attenuations = [
        (Attenuation::_0dB, "0dB"),
        (Attenuation::_2p5dB, "2.5dB"),
        (Attenuation::_6dB, "6dB"),
        (Attenuation::_11dB, "11dB"),
    ];

    for (atten, name) in attenuations {
        let init_code = Efuse::rtc_calib_init_code(AdcCalibUnit::ADC1, atten);
        let cal_code = Efuse::rtc_calib_cal_code(AdcCalibUnit::ADC1, atten);
        let cal_mv = Efuse::rtc_calib_cal_mv(AdcCalibUnit::ADC1, atten);
        info!(
            "ADC1 {}: init_code={}, cal_code={}, cal_mv={}",
            name, init_code, cal_code, cal_mv,
        );
    }
}

#[embassy_executor::task]
async fn log_voltage_changes_task(adc: AdcHandle) {
    loop {
        if let Some(state) = adc.state() {
            info!(
                "Battery voltage: {}mV, Boost voltage: {}mV",
                state.battery_voltage, state.boost_voltage
            );
        }
        Timer::after(Duration::from_secs(10)).await;
    }
}

#[embassy_executor::task]
async fn log_power_state_changes_task(mut receiver: PowerStateReceiver) {
    loop {
        let stats = receiver.changed().await.clone();
        stats.dump();
    }
}
