#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

mod config;
mod driver;
mod rtc;
mod wifi;
mod ntp;

use alloc::rc::Rc;
use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::rwlock::RwLock;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use panic_rtt_target as _;
use static_cell::StaticCell;
use embassy_sync::once_lock::OnceLock;
use esp_hal::gpio::{Input, InputConfig};

use mainboard::board::{A0Pin, Board, D0Pin, acquire_i2c_bus, init_i2c_bus};
use mainboard::tasks::{
    PowerStateReceiver, spawn_ext_interrupt_task, spawn_power_controller
};
use mainboard::create_board;
use mainboard::power::PowerControllerIO;
use crate::driver::{ClockDriver, spawn_clock_task};
use crate::ntp::sync_time_with_ntp;
use crate::rtc::rtc_handler;
use crate::wifi::{WifiResources, initialize_wifi};

extern crate alloc;

static ESP_RADIO_INIT: StaticCell<esp_radio::Controller<'static>> = StaticCell::new();
static ESP_WIFI_RES: StaticCell<WifiResources> = StaticCell::new();
static CLOCK_DRIVER: OnceLock<ClockDriver> = OnceLock::new();

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.2.0

    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    info!("Embassy initialized!");

    let board = create_board!(peripherals);

    init_i2c_bus(peripherals.I2C0, board.Sda, board.Scl).expect("Failed to initialize I2C bus");

    info!("Initializing WiFi...");
    let mut rng = esp_hal::rng::Rng::new();
    let radio_init = ESP_RADIO_INIT.init(esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller"));
    let wifi_res = ESP_WIFI_RES.init(initialize_wifi(spawner, radio_init, peripherals.WIFI, &mut rng).await);
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

    spawner.spawn(sync_time_with_ntp(wifi_res)).expect("Failed to start ntp sync task");

    spawner.spawn(rtc_handler()).expect("Cannot start RTC handling task");

    spawn_clock_task(&spawner, board.Motor0, board.Motor1, power);

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
        //CLOCK_DRIVER.get().await.push_forward(1);
    }
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


