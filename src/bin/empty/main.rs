#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use mainboard::board::{acquire_i2c_bus, init_i2c_bus, Board};
use mainboard::create_board;
use mainboard::idle_monitor::{self, IdleWindowTracker};
use mainboard::tasks::{spawn_ext_interrupt_task, spawn_power_controller, PowerStateReceiver};

use defmt::info;
use embassy_executor::Spawner;

use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use mainboard::power::PowerControllerIO;
use panic_rtt_target as _;

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start_with_idle_hook(
        timg0.timer0,
        sw_interrupt.software_interrupt0,
        idle_monitor::idle_hook,
    );

    info!("Embassy initialized!");

    let board = create_board!(peripherals);

    init_i2c_bus(peripherals.I2C0, board.Sda, board.Scl).expect("Failed to initialize I2C bus");

    let _rng = esp_hal::rng::Rng::new();
    let radio_init = esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller");
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(&radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize WIFI controller");

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
    spawner
        .spawn(idle_metrics_task())
        .expect("Failed to spawn idle_metrics_task");

    spawn_ext_interrupt_task(&spawner, board.GlobalInt, power, None);

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
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
async fn idle_metrics_task() {
    let mut tracker = IdleWindowTracker::new();

    loop {
        Timer::after_millis(idle_monitor::DEFAULT_REPORT_INTERVAL_MS).await;

        let sample = tracker.sample_and_reset();
        let busy_whole = sample.busy_permille / 10;
        let busy_tenths = sample.busy_permille % 10;
        let idle_whole = sample.idle_permille / 10;
        let idle_tenths = sample.idle_permille % 10;
        let idle_ms = idle_monitor::ticks_to_millis(sample.idle_ticks);
        let window_ms = idle_monitor::ticks_to_millis(sample.window_ticks);

        info!(
            "CPU: busy {}.{}%, idle {}.{}% ({} ms idle / {} ms window)",
            busy_whole, busy_tenths, idle_whole, idle_tenths, idle_ms, window_ms,
        );
    }
}
