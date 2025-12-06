#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod config;
mod server;
mod wifi;

use esp_hal::analog::adc::AdcConfig;
use mainboard::board::{acquire_i2c_bus, init_i2c_bus, Board};
use mainboard::tasks::{
    spawn_adc_task,
    spawn_digital_io,
    spawn_ext_interrupt_task,
    spawn_power_controller,
    spawn_uart_tasks,
    AdcHandle,
    PowerResponse,
    PowerStateReceiver,
    VoltageMonitorCalibrationConfig,
    DigitalPinID,
    PinMode,
};
use mainboard::create_board;
use mainboard::power::{PowerControllerIO, PowerControllerMode};

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_hal::{clock::CpuClock, rtc_cntl::Rtc};
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use panic_rtt_target as _;
use static_cell::StaticCell;
use crate::server::ShutdownHandle;

// StaticCell for WiFi controller
static ESP_WIFI_CTRL: StaticCell<esp_wifi::EspWifiController<'static>> = StaticCell::new();
static SHUTDOWN_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // Initialize RTT for logging
    rtt_target::rtt_init_defmt!();

    // Configure and initialize hardware
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let board = create_board!(peripherals);

    // Initialize heap allocator
    esp_alloc::heap_allocator!(size: 64 * 1024);

    // Initialize embassy time
    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);
    info!("Embassy initialized!");

    init_i2c_bus(peripherals.I2C0, board.Sda, board.Scl).expect("Failed to initialize I2C bus");

    // Initialize RNG and timer for WiFi
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    // Initialize esp-radio controller
    let esp_wifi_ctrl = ESP_WIFI_CTRL.init(esp_wifi::init(timer1.timer0, rng).unwrap());

    let power_config = Default::default();
    let power_io = PowerControllerIO {
        charger_i2c: acquire_i2c_bus(),
        pcf8574_i2c: acquire_i2c_bus(),
        boost_converter_enable: board.BoostEn,
    };
    let power = spawn_power_controller(&spawner, power_config, power_io);
    let power_receiver = power.state_receiver().expect("Failed to get power state receiver");
    spawner.spawn(log_power_state_changes_task(power_receiver)).expect("Failed to spawn log_power_state_changes_task");

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
    spawner.spawn(log_voltage_changes_task(adc)).expect("Failed to spawn log_voltage_changes_task");

    spawn_ext_interrupt_task(&spawner, board.GlobalInt, power);

    // Initialize UART
    info!("Initializing UART...");
    let uart = esp_hal::uart::Uart::new(
        peripherals.UART0,
        esp_hal::uart::Config::default(),
    ).unwrap()
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
        wifi::initialize_wifi(spawner, esp_wifi_ctrl, peripherals.WIFI, &mut rng).await;
    info!("WiFi initialized!");

    // Initialize simple output
    let digital = spawn_digital_io(&spawner, board.D0, board.D1, board.D2, board.D3, board.D4);

    // Start the web server
    info!("Starting web server...");
    let shutdown_handle = ShutdownHandle::new(&SHUTDOWN_SIGNAL);
    server::run_server(spawner, &wifi_resources, power, adc, digital, uart_handle, shutdown_handle).await;
    info!("Web server started!");

    // Main loop
    loop {
        match select(Timer::after(Duration::from_secs(10)), SHUTDOWN_SIGNAL.wait()).await {
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
        digital.set_mode(pin, PinMode::OpenDrain).await;
        digital.set(pin, true).await;
    }

    match power.set_mode(PowerControllerMode::Charging).await {
        PowerResponse::Ok => info!("Charger set to Charging mode"),
        PowerResponse::Err(e) => info!("Failed to set Charging mode: {:?}", e),
    }

    info!("Entering deep sleep (shutdown)");
    let mut rtc = Rtc::new(peripherals.LPWR);
    rtc.sleep_deep(&[]);
}

#[embassy_executor::task]
async fn log_voltage_changes_task(adc: AdcHandle) {
    loop {
        if let Some(state) = adc.state() {
            info!(
                "Battery voltage: {}mV, Boost voltage: {}mV",
                state.battery_voltage,
                state.boost_voltage
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
