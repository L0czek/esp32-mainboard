use defmt::{error, info, warn};
use embassy_time::{Duration, Instant, Ticker, Timer};
use esp_hal::peripherals::UART0;
use esp_hal::uart::Uart;

use crate::config::{ONESHOT_CONVERSION_MS, TEMP_BATCH_SIZE, TEMP_COLLECTION_INTERVAL_MS};
use crate::mqtt::publish_temperature_sensor;
use crate::mqtt::sensors::temp::TempPacket;
use mainboard::board::{D0Pin, U0RxPin, U0TxPin};
use mainboard::tmp107::{Tmp107, MAX_SENSORS};

pub struct TemperatureCollectionIo {
    pub uart: UART0<'static>,
    pub tx_pin: U0TxPin,
    pub rx_pin: U0RxPin,
    pub dir_pin: D0Pin,
}

#[embassy_executor::task]
pub async fn temperature_collection_task(io: TemperatureCollectionIo) {
    let uart = Uart::new(
        io.uart,
        esp_hal::uart::Config::default().with_baudrate(115200),
    )
    .expect("UART0 init failed")
    .with_tx(io.tx_pin)
    .with_rx(io.rx_pin)
    .with_dtr(io.dir_pin)
    .with_rs485()
    .into_async();

    let (rx, tx) = uart.split();

    let mut driver = match Tmp107::init(tx, rx).await {
        Ok(d) => d,
        Err(e) => {
            error!("TMP107 init failed: {:?}", e);
            return;
        }
    };

    let sensor_count = driver.sensor_count() as usize;

    if let Err(e) = driver.shutdown().await {
        error!("TMP107 shutdown failed: {:?}", e);
        return;
    }

    info!(
        "Temperature collection: {} sensors, {}ms interval, batch {}",
        sensor_count, TEMP_COLLECTION_INTERVAL_MS, TEMP_BATCH_SIZE,
    );

    let mut ticker = Ticker::every(Duration::from_millis(TEMP_COLLECTION_INTERVAL_MS));
    let mut read_buf = [0u16; MAX_SENSORS];
    let mut batch = [[0u16; TEMP_BATCH_SIZE]; MAX_SENSORS];
    let mut sample_index: usize = 0;
    let mut first_timestamp_ms: u32 = 0;

    loop {
        ticker.next().await;

        if let Err(e) = driver.trigger_one_shot().await {
            warn!("TMP107 one-shot trigger failed: {:?}", e);
            continue;
        }

        Timer::after_millis(ONESHOT_CONVERSION_MS).await;

        let count = match driver.read_all_temperatures(&mut read_buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("TMP107 read failed: {:?}", e);
                continue;
            }
        };

        if let Err(e) = driver.show_address_leds().await {
            warn!("TMP107 show address LEDs failed: {:?}", e);
        }

        let now = Instant::now().as_millis() as u32;

        if sample_index == 0 {
            first_timestamp_ms = now;
        }

        for sensor in 0..count {
            batch[sensor][sample_index] = read_buf[sensor];
        }
        sample_index += 1;

        if sample_index >= TEMP_BATCH_SIZE {
            for (sensor, samples) in batch.iter().enumerate().take(count) {
                let sensor_id = (sensor + 1) as u8;
                let packet = match TempPacket::from_slice(
                    sensor_id,
                    first_timestamp_ms,
                    now,
                    &samples[..TEMP_BATCH_SIZE],
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("TMP107 packet error sensor {}: {:?}", sensor_id, e,);
                        continue;
                    }
                };

                if publish_temperature_sensor(packet).is_err() {
                    warn!("Dropping temp packet: queue full");
                }
            }
            sample_index = 0;
        }
    }
}
