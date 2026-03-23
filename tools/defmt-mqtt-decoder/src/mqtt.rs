use std::time::Duration;

use anyhow::Result;
use rumqttc::{Client, Connection, Event, MqttOptions, Packet, QoS};

pub trait PayloadHandler {
    fn handle_payload(&mut self, payload: &[u8]) -> Result<()>;
}

pub struct SubscriberConfig<'a> {
    pub client_id: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub topic: &'a str,
    pub keep_alive_secs: u64,
    pub request_capacity: usize,
}

pub fn connect(config: &SubscriberConfig<'_>) -> (Client, Connection) {
    let mut options = MqttOptions::new(config.client_id, config.host, config.port);
    options.set_keep_alive(Duration::from_secs(config.keep_alive_secs));
    Client::new(options, config.request_capacity)
}

pub fn run_subscription(
    client: &mut Client,
    connection: &mut Connection,
    topic: &str,
    handler: &mut impl PayloadHandler,
) -> Result<()> {
    client.subscribe(topic, QoS::AtMostOnce)?;

    for notification in connection.iter() {
        let event = notification?;
        forward_event(handler, &event, topic)?;
    }

    Ok(())
}

pub fn forward_event(handler: &mut impl PayloadHandler, event: &Event, topic: &str) -> Result<()> {
    if let Event::Incoming(Packet::Publish(publish)) = event
        && publish.topic == topic
    {
        handler.handle_payload(publish.payload.as_ref())?;
    }

    Ok(())
}
