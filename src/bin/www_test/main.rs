#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod config;
mod html;
mod server;
mod simple_output;
mod wifi;

use core::borrow::Borrow;

use esp_hal::analog::adc::AdcConfig;
use mainboard::board::{acquire_i2c_bus, init_i2c_bus, Board, ADC_STATE, POWER_STATE};
use mainboard::create_board;
use mainboard::power::PowerControllerIO;
use mainboard::tasks::{handle_ext_interrupt_line, handle_power_controller, monitor_voltages};
use simple_output::initialize_simple_output;

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use panic_rtt_target as _;
use static_cell::StaticCell;

// StaticCell for WiFi controller
static ESP_WIFI_CTRL: StaticCell<esp_wifi::EspWifiController<'static>> = StaticCell::new();

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
    let _ = spawner.spawn(handle_power_controller(power_config, power_io));
    let _ = spawner.spawn(log_power_state_changes());

    let adc_config = AdcConfig::new();
    let calibration = Default::default();
    let _ = spawner.spawn(monitor_voltages(
        peripherals.ADC1,
        adc_config,
        calibration,
        board.BatVol,
        board.BoostVol,
    ));
    let _ = spawner.spawn(log_voltage_changes());

    let _ = spawner.spawn(handle_ext_interrupt_line(board.GlobalInt));

    // Initialize WiFi in mixed mode (AP + STA)
    info!("Initializing WiFi...");
    let wifi_resources =
        wifi::initialize_wifi(spawner, esp_wifi_ctrl, peripherals.WIFI, &mut rng).await;
    info!("WiFi initialized!");

    // Initialize simple output
    initialize_simple_output(&spawner, board.D0, board.D1);

    // Start the web server
    info!("Starting web server...");
    server::run_server(spawner, &wifi_resources).await;
    info!("Web server started!");

    // Main loop
    loop {
        info!(
            "Server running... AP IP: {:?}, STA IP: {:?}",
            wifi_resources.ap_stack.config_v4().map(|c| c.address),
            wifi_resources.sta_stack.config_v4().map(|c| c.address)
        );
        Timer::after(Duration::from_secs(10)).await;
    }
}

#[embassy_executor::task]
async fn log_voltage_changes() {
    loop {
        if let Some(state) = ADC_STATE.try_get() {
            info!("Battery voltage: {}mV, Boost voltage: {}mV", state.battery_voltage, state.boost_voltage);
        }
        Timer::after(Duration::from_secs(10)).await;
    }
}

#[embassy_executor::task]
async fn log_power_state_changes() {
    let mut receiver = POWER_STATE.receiver().unwrap();
    loop {
        let stats = receiver.changed().await.borrow().clone();
        stats.dump();
    }
}
