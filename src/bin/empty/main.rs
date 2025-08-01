#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use core::borrow::Borrow;

use esp_hal::analog::adc::AdcConfig;
use mainboard::board::{acquire_i2c_bus, init_i2c_bus, Board, ADC_STATE, POWER_STATE};
use mainboard::create_board;

use defmt::info;
use embassy_executor::Spawner;

use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use mainboard::power::PowerControllerIO;
use mainboard::tasks::{handle_ext_interrupt_line, handle_power_controller, monitor_voltages};
use panic_rtt_target as _;

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.5.0

    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let board = create_board!(peripherals);

    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    init_i2c_bus(peripherals.I2C0, board.Sda, board.Scl).expect("Failed to initialize I2C bus");

    let rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);
    let wifi_init =
        esp_wifi::init(timer1.timer0, rng).expect("Failed to initialize WIFI/BLE controller");
    let (mut _wifi_controller, _interfaces) = esp_wifi::wifi::new(&wifi_init, peripherals.WIFI)
        .expect("Failed to initialize WIFI controller");

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

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-rc.0/examples/src/bin
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
