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
use mainboard::tmp107::{Tmp107, Tmp107Error, MAX_SENSORS, ONESHOT_CONVERSION_MS};
use panic_rtt_target as _;

extern crate alloc;

const LED_STEP_MS: u64 = 150;
const ADDRESS_HOLD_MS: u64 = 5000;
const LOOP_PAUSE_MS: u64 = 1000;

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
    // Board-specific UART0 wiring for TMP107 SMAART wire:
    // D0 drives RS485 direction via UART DTR, so the HAL controls TX/RX turn-around.
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

    if let Err(error) = clear_alert_gpio_outputs(&mut driver).await {
        warn!(
            "TMP107 clear ALERT GPIO outputs failed during startup: {:?}",
            error
        );
    }

    info!(
        "TMP107 sensors address initialize: {} sensors discovered",
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

        match driver.address_initialize().await {
            Ok(count) => {
                info!(
                    "TMP107 sensors address initialize: {} sensors discovered",
                    count
                );
            }
            Err(error) => {
                warn!("TMP107 sensors address initialize error: {:?}", error);
            }
        }

        Timer::after_millis(ONESHOT_CONVERSION_MS).await;

        if let Err(error) = log_temperatures(&mut driver, &mut read_buf).await {
            warn!("TMP107 temperature read failed: {:?}", error);
            Timer::after_millis(LOOP_PAUSE_MS).await;
            continue;
        }

        if let Err(error) = blink_alert_gpio_pattern(&mut driver).await {
            warn!("TMP107 ALERT GPIO pattern failed: {:?}", error);
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

/// Blink ALERT1/ALERT2 as GPIO outputs.
///
/// On this test board the ALERT pins are connected to LEDs, so this pattern is visible.
async fn blink_alert_gpio_pattern(driver: &mut Tmp107) -> Result<(), Tmp107Error> {
    clear_alert_gpio_outputs(driver).await?;

    for address in 1..=driver.sensor_count() {
        info!("ALERT GPIO pattern: sensor {} ALERT1 high", address);
        driver.set_gpio_outputs(address, true, false).await?;
        driver.trigger_one_shot().await?;
        Timer::after_millis(LED_STEP_MS).await;
        driver.set_gpio_outputs(address, false, false).await?;
        driver.trigger_one_shot().await?;
    }

    for address in (1..=driver.sensor_count()).rev() {
        info!("ALERT GPIO pattern: sensor {} ALERT2 high", address);
        driver.set_gpio_outputs(address, false, true).await?;
        driver.trigger_one_shot().await?;
        Timer::after_millis(LED_STEP_MS).await;
        driver.set_gpio_outputs(address, false, false).await?;
        driver.trigger_one_shot().await?;
    }

    info!("ALERT GPIO pattern: expose lower address bits");
    driver.expose_lower_address_bits_on_gpio().await?;
    driver.trigger_one_shot().await?;
    Timer::after_millis(ADDRESS_HOLD_MS).await;
    clear_alert_gpio_outputs(driver).await
}

async fn clear_alert_gpio_outputs(driver: &mut Tmp107) -> Result<(), Tmp107Error> {
    for address in 1..=driver.sensor_count() {
        driver.set_gpio_outputs(address, false, false).await?;
        driver.trigger_one_shot().await?;
    }

    Ok(())
}

fn raw_temperature_to_millicelsius(raw_value: u16) -> i32 {
    let raw_units = i16::from_le_bytes(raw_value.to_le_bytes()) >> 2;
    (i32::from(raw_units) * 1_000) / 64
}
