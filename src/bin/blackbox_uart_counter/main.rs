#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::uart::UartTx;
use mainboard::board::Board;
use mainboard::create_board;
use mainboard::idle_monitor::{self, IdleWindowTracker};
use panic_rtt_target as _;

const BLACKBOX_BAUD_RATE: u32 = 3_000_000;
const SEND_INTERVAL_MS: u64 = 1;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 32768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start_with_idle_hook(
        timg0.timer0,
        sw_interrupt.software_interrupt0,
        idle_monitor::idle_hook,
    );
    info!("Embassy initialized for blackbox UART counter");

    spawner
        .spawn(idle_metrics_task())
        .expect("Failed to spawn idle_metrics_task");

    let board = create_board!(peripherals);

    let mut tx = UartTx::new(
        peripherals.UART1,
        esp_hal::uart::Config::default().with_baudrate(BLACKBOX_BAUD_RATE),
    )
    .expect("UART1 init failed")
    .with_tx(board.D4);

    let mut counter: u32 = 0;
    loop {
        let payload = counter.to_le_bytes();
        write_all(&mut tx, &payload);
        counter = counter.wrapping_add(1);
        Timer::after_millis(SEND_INTERVAL_MS).await;
    }
}

fn write_all(tx: &mut UartTx<'static, esp_hal::Blocking>, buf: &[u8]) {
    let mut remaining = buf;

    while !remaining.is_empty() {
        while !tx.write_ready() {
            // Busy wait until FIFO has room to preserve packet boundaries.
        }

        let written = tx.write(remaining).expect("UART1 write failed");
        remaining = &remaining[written..];
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
