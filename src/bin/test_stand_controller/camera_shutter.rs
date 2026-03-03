use defmt::{info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Output;

const SHUTTER_PRESS_DURATION_MS: u64 = 200;

static SHUTTER_CHANNEL: Channel<CriticalSectionRawMutex, (), 4> = Channel::new();

pub fn trigger_shutter() {
    if SHUTTER_CHANNEL.try_send(()).is_err() {
        warn!("Shutter channel full, dropping request");
    }
}

#[embassy_executor::task]
pub async fn camera_shutter_task(mut pin: Output<'static>) {
    info!("Camera shutter task started");

    loop {
        SHUTTER_CHANNEL.receive().await;
        info!("Camera shutter triggered");
        pin.set_high();
        Timer::after(Duration::from_millis(SHUTTER_PRESS_DURATION_MS)).await;
        pin.set_low();
        Timer::after(Duration::from_millis(SHUTTER_PRESS_DURATION_MS)).await;
    }
}
