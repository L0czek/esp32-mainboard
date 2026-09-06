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
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub keep_alive_secs: u64,
    pub request_capacity: usize,
}

pub fn connect(config: &SubscriberConfig<'_>) -> (Client, Connection) {
    let options = build_mqtt_options(config);
    Client::new(options, config.request_capacity)
}

fn build_mqtt_options(config: &SubscriberConfig<'_>) -> MqttOptions {
    let mut options = MqttOptions::new(config.client_id, config.host, config.port);
    options.set_keep_alive(Duration::from_secs(config.keep_alive_secs));

    if let (Some(username), Some(password)) = (config.username, config.password) {
        options.set_credentials(username, password);
    }

    options
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config<'a>() -> SubscriberConfig<'a> {
        SubscriberConfig {
            client_id: "decoder",
            host: "broker.local",
            port: 1883,
            topic: "log/defmt",
            username: None,
            password: None,
            keep_alive_secs: 5,
            request_capacity: 16,
        }
    }

    #[test]
    fn mqtt_options_leave_credentials_empty_by_default() {
        let options = build_mqtt_options(&test_config());

        assert!(options.credentials().is_none());
    }

    #[test]
    fn mqtt_options_apply_username_and_password_when_both_are_present() {
        let mut config = test_config();
        config.username = Some("user");
        config.password = Some("secret");

        let credentials = build_mqtt_options(&config).credentials().unwrap();

        assert_eq!(credentials.username, "user");
        assert_eq!(credentials.password, "secret");
    }
}
