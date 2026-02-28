use defmt::{error, info, warn};
use embassy_time::{Duration, Instant, Ticker};
use esp_hal::peripherals::UART0;
use esp_hal::uart::Uart;

use crate::config::TEMP_COLLECTION_INTERVAL_MS;
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

    info!(
        "Temperature collection: {} sensors, {}ms interval",
        driver.sensor_count(),
        TEMP_COLLECTION_INTERVAL_MS,
    );

    let mut ticker = Ticker::every(Duration::from_millis(TEMP_COLLECTION_INTERVAL_MS));
    let mut buf = [0u16; MAX_SENSORS];

    loop {
        ticker.next().await;

        let timestamp_ms = Instant::now().as_millis() as u32;

        let count = match driver.read_all_temperatures(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("TMP107 read failed: {:?}", e);
                continue;
            }
        };

        for (i, &value) in buf.iter().enumerate().take(count) {
            let sensor_id = (i + 1) as u8;
            let packet = match TempPacket::from_slice(sensor_id, timestamp_ms, &[value]) {
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
    }
}
