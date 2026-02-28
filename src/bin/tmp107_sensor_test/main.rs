#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_time::Timer;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::uart::Uart;
use mainboard::board::Board;
use mainboard::create_board;
use mainboard::tmp107::{Tmp107, Tmp107Error, MAX_SENSORS};
use panic_rtt_target as _;

extern crate alloc;

const ONESHOT_CONVERSION_MS: u64 = 20;
const LED_STEP_MS: u64 = 150;
const ADDRESS_HOLD_MS: u64 = 600;
const LOOP_PAUSE_MS: u64 = 300;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "Main owns the temporary sensor buffer and UART setup during startup."
)]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 32768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    info!("Embassy initialized for TMP107 sensor test");

    let board = create_board!(peripherals);
    let uart = Uart::new(
        peripherals.UART0,
        esp_hal::uart::Config::default().with_baudrate(115200),
    )
    .expect("UART0 init failed")
    .with_tx(board.U0Tx)
    .with_rx(board.U0Rx)
    .with_dtr(board.D0)
    .with_rs485()
    .into_async();

    let (rx, tx) = uart.split();
    let mut driver = match Tmp107::init(tx, rx).await {
        Ok(driver) => driver,
        Err(error) => {
            error!("TMP107 init failed: {:?}", error);
            loop {
                Timer::after_millis(1_000).await;
            }
        }
    };

    if let Err(error) = driver.shutdown().await {
        error!("TMP107 shutdown configuration failed: {:?}", error);
        loop {
            Timer::after_millis(1_000).await;
        }
    }

    if let Err(error) = clear_leds(&mut driver).await {
        warn!("TMP107 clear LEDs failed during startup: {:?}", error);
    }

    info!(
        "TMP107 sensor test ready: {} sensors discovered",
        driver.sensor_count()
    );

    let mut cycle: u32 = 0;
    let mut read_buf = [0u16; MAX_SENSORS];

    loop {
        cycle = cycle.wrapping_add(1);
        info!("TMP107 test cycle {} starting", cycle);
        
        if let Err(error) = driver.trigger_one_shot().await {
            warn!("TMP107 one-shot trigger failed: {:?}", error);
            Timer::after_millis(LOOP_PAUSE_MS).await;
            continue;
        }

        Timer::after_millis(ONESHOT_CONVERSION_MS).await;

        if let Err(error) = log_temperatures(&mut driver, &mut read_buf).await {
            warn!("TMP107 temperature read failed: {:?}", error);
            Timer::after_millis(LOOP_PAUSE_MS).await;
            continue;
        }

        if let Err(error) = blink_led_pattern(&mut driver).await {
            warn!("TMP107 LED pattern failed: {:?}", error);
        }

        Timer::after_millis(LOOP_PAUSE_MS).await;
    }
}

async fn log_temperatures(
    driver: &mut Tmp107,
    read_buf: &mut [u16; MAX_SENSORS],
) -> Result<(), Tmp107Error> {
    let count = driver.read_all_temperatures(read_buf).await?;
    info!("TMP107 captured {} temperature readings", count);

    for (index, raw_value) in read_buf[..count].iter().copied().enumerate() {
        let milli_celsius = raw_temperature_to_millicelsius(raw_value);
        info!(
            "TMP107 sensor {}: raw {:#06x}, {} mC",
            index + 1,
            raw_value,
            milli_celsius,
        );
    }

    Ok(())
}

async fn blink_led_pattern(driver: &mut Tmp107) -> Result<(), Tmp107Error> {
    clear_leds(driver).await?;

    for address in 1..=driver.sensor_count() {
        info!("LED pattern: sensor {} ALERT1", address);
        driver.set_leds(address, true, false).await?;
        Timer::after_millis(LED_STEP_MS).await;
        driver.set_leds(address, false, false).await?;
    }

    for address in (1..=driver.sensor_count()).rev() {
        info!("LED pattern: sensor {} ALERT2", address);
        driver.set_leds(address, false, true).await?;
        Timer::after_millis(LED_STEP_MS).await;
        driver.set_leds(address, false, false).await?;
    }

    info!("LED pattern: address bits");
    driver.show_address_leds().await?;
    Timer::after_millis(ADDRESS_HOLD_MS).await;
    clear_leds(driver).await
}

async fn clear_leds(driver: &mut Tmp107) -> Result<(), Tmp107Error> {
    for address in 1..=driver.sensor_count() {
        driver.set_leds(address, false, false).await?;
    }

    Ok(())
}

fn raw_temperature_to_millicelsius(raw_value: u16) -> i32 {
    let raw_units = i16::from_le_bytes(raw_value.to_le_bytes()) >> 2;
    (i32::from(raw_units) * 1_000) / 64
}
