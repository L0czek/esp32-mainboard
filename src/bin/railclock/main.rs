#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod config;
mod driver;
mod rtc;
mod wifi;
mod ntp;

use embassy_futures::select::select;
use embassy_sync::once_lock::OnceLock;
use esp_hal::gpio::{Input, InputConfig};
use mainboard::board::{A0Pin, Board, D0Pin, acquire_i2c_bus, init_i2c_bus};
use mainboard::tasks::{
    PowerStateReceiver, spawn_ext_interrupt_task, spawn_power_controller
};
use mainboard::create_board;

use defmt::info;
use embassy_executor::Spawner;

use embassy_time::{Duration, Instant, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use mainboard::power::PowerControllerIO;
use panic_rtt_target as _;
use static_cell::StaticCell;
use crate::driver::{ClockDriver, ClockDriverState, spawn_clock_task};
use crate::ntp::sync_time_with_ntp;
use crate::wifi::WifiResources;

extern crate alloc;

static ESP_WIFI_CTRL: StaticCell<esp_wifi::EspWifiController<'static>> = StaticCell::new();
static CLOCK_DRIVER: OnceLock<ClockDriver> = OnceLock::new();
static WIFI_RESOURCES: OnceLock<WifiResources> = OnceLock::new();

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

    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    let esp_wifi_ctrl = ESP_WIFI_CTRL.init(esp_wifi::init(timer1.timer0, rng).unwrap());

    info!("Initializing WiFi...");
    let wifi_resources = wifi::initialize_wifi(spawner, esp_wifi_ctrl, peripherals.WIFI, &mut rng).await;
    wifi_resources.sta_stack.wait_link_up().await;
    WIFI_RESOURCES.get_or_init(move || wifi_resources);
    info!("WiFi initialized!");

    CLOCK_DRIVER.get_or_init(|| ClockDriver::new());

    let power_config = Default::default();
    let power_io = PowerControllerIO {
        charger_i2c: acquire_i2c_bus(),
        pcf8574_i2c: acquire_i2c_bus(),
        boost_converter_enable: board.BoostEn,
    };
    let power = spawn_power_controller(&spawner, power_config, power_io);
    let power_receiver = power.state_receiver().expect("Failed to get power state receiver");
    spawner.spawn(log_power_state_changes_task(power_receiver)).expect("Failed to spawn log_power_state_changes_task");

    spawn_ext_interrupt_task(&spawner, board.GlobalInt, power);

    spawner.spawn(sync_time_with_ntp()).expect("Failed to start ntp sync task");

    spawn_clock_task(&spawner, board.Motor0, board.Motor1, power);

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
        //CLOCK_DRIVER.get().await.push_forward(1);
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-rc.0/examples/src/bin
}

#[embassy_executor::task]
async fn log_power_state_changes_task(mut receiver: PowerStateReceiver) {
    loop {
        let stats = receiver.changed().await.clone();
        stats.dump();
    }
}

#[embassy_executor::task]
async fn listen_on_buttons(bt0: A0Pin, bt1: D0Pin) {
    let mut p0 = Input::new(bt0, InputConfig::default().with_pull(esp_hal::gpio::Pull::Up));
    let mut p1 = Input::new(bt1, InputConfig::default().with_pull(esp_hal::gpio::Pull::Up));

    loop {
        match select(p0.wait_for_low(), p1.wait_for_low()).await {
            _ => {
                CLOCK_DRIVER.get().await.push_forward(1);
                Timer::after_millis(600).await;
            }
        }
    }
}


