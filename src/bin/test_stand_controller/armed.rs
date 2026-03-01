// TODO: In the future, pin ownership will move to the rocket sequencing task,
// which needs direct control of the safety switch pin during launch sequences.
// This standalone task will be removed once sequencing is implemented.

use core::sync::atomic::{AtomicU8, Ordering};

use defmt::{info, warn};
use embassy_time::Instant;
use esp_hal::gpio::Input;

use crate::mqtt::sensors::digital::ArmedPacket;

static LAST_ARMED_VALUE: AtomicU8 = AtomicU8::new(0);

pub fn init_armed_state(pin: &Input<'_>) {
    let value = pin.is_high() as u8;
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    info!("Armed switch initial state: {}", value);
}

#[embassy_executor::task]
pub async fn armed_monitor_task(mut pin: Input<'static>) {
    loop {
        pin.wait_for_any_edge().await;
        publish_armed_state(&pin);
    }
}

pub fn republish_armed_state() {
    let value = LAST_ARMED_VALUE.load(Ordering::Relaxed);
    let packet = ArmedPacket::new(timestamp_ms(), value);
    let _ = crate::mqtt::publish_armed_sensor(packet);
}

fn publish_armed_state(pin: &Input<'_>) {
    let value = pin.is_high() as u8;
    let ts = timestamp_ms();
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    let packet = ArmedPacket::new(ts, value);

    info!("Armed switch: {}", value);
    if crate::mqtt::publish_armed_sensor(packet).is_err() {
        warn!("Dropping armed packet: outbound queue full");
    }
    crate::blackbox::send_to_blackbox(crate::blackbox::BlackboxPacket::Digital {
        timestamp_ms: ts,
        value,
    });
}

fn timestamp_ms() -> u32 {
    Instant::now().as_millis() as u32
}
