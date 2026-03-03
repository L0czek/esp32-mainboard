use alloc::string::String;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

#[derive(Clone)]
pub enum OutgoingMessage {
    Publish {
        topic: &'static str,
        payload: String,
        retain: bool,
    },
}

pub static OUTGOING_CH: Channel<CriticalSectionRawMutex, OutgoingMessage, 16> = Channel::new();

/// Try to publish a message by enqueuing it on the outgoing channel.
/// Returns `Ok(())` if enqueued, `Err(())` if the queue is full.
pub fn mqtt_publish(topic: &'static str, payload: &str, retain: bool) -> Result<(), ()> {
    OUTGOING_CH
        .try_send(OutgoingMessage::Publish {
            topic,
            payload: String::from(payload),
            retain,
        })
        .map_err(|_| ())
}
