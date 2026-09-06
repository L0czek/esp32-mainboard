use defmt::{info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::Output;

const SHUTTER_PRESS_DURATION_MS: u64 = 200;
const MAX_RECORD_DURATION_MS: u64 = 60 * 1000; // 1 minutes
const TOLLERANCE_MS: u64 = 2000; // Tolerance for timing issues when checking if already recording

pub enum CameraShutterCommand {
    Record,
    Stop,
}

static SHUTTER_CHANNEL: Channel<CriticalSectionRawMutex, CameraShutterCommand, 4> = Channel::new();

pub fn send_camera_command(command: CameraShutterCommand) {
    if SHUTTER_CHANNEL.try_send(command).is_err() {
        warn!("Shutter channel full, dropping request");
    }
}

async fn trigger_shutter(pin: &mut Output<'static>) {
    pin.set_high();
    Timer::after(Duration::from_millis(SHUTTER_PRESS_DURATION_MS)).await;
    pin.set_low();
    Timer::after(Duration::from_millis(SHUTTER_PRESS_DURATION_MS)).await;
}

#[embassy_executor::task]
pub async fn camera_shutter_task(mut pin: Output<'static>) {
    info!("Camera shutter task started");
    let mut recording_since = None;

    loop {
        let command = SHUTTER_CHANNEL.receive().await;

        match command {
            CameraShutterCommand::Record => {
                if let Some(since) = recording_since {
                    warn!(
                        "Received RECORD command while already recording since {:?}",
                        since
                    );
                }

                info!("Starting recording");
                recording_since = Some(Instant::now());
                trigger_shutter(&mut pin).await;
            }
            CameraShutterCommand::Stop => {
                if let Some(since) = recording_since {
                    if since + Duration::from_millis(MAX_RECORD_DURATION_MS - TOLLERANCE_MS)
                        > Instant::now()
                    {
                        info!("Stopping recording");
                        trigger_shutter(&mut pin).await;
                        recording_since = None;
                        continue;
                    }
                }
                recording_since = None;
                info!("Not currently recording, ignoring STOP command");
            }
        }
    }
}
