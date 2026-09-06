use anyhow::Result;
use rumqttc::{Event, Packet, Publish, QoS};

use defmt_mqtt_decoder::mqtt::{PayloadHandler, forward_event};

#[derive(Default)]
struct RecordingHandler {
    payloads: Vec<Vec<u8>>,
}

impl PayloadHandler for RecordingHandler {
    fn handle_payload(&mut self, payload: &[u8]) -> Result<()> {
        self.payloads.push(payload.to_vec());
        Ok(())
    }
}

#[test]
fn forwards_matching_publish_payloads_without_modifying_bytes() {
    let mut handler = RecordingHandler::default();
    let first = Event::Incoming(Packet::Publish(Publish::new(
        "log/defmt",
        QoS::AtMostOnce,
        vec![1u8, 2, 3],
    )));
    let second = Event::Incoming(Packet::Publish(Publish::new(
        "log/defmt",
        QoS::AtMostOnce,
        vec![4u8, 5],
    )));

    forward_event(&mut handler, &first, "log/defmt").unwrap();
    forward_event(&mut handler, &second, "log/defmt").unwrap();

    assert_eq!(handler.payloads, vec![vec![1, 2, 3], vec![4, 5]]);
}

#[test]
fn ignores_publish_events_for_other_topics() {
    let mut handler = RecordingHandler::default();
    let event = Event::Incoming(Packet::Publish(Publish::new(
        "status/state",
        QoS::AtMostOnce,
        vec![9u8],
    )));

    forward_event(&mut handler, &event, "log/defmt").unwrap();

    assert!(handler.payloads.is_empty());
}
