use crate::mqtt::codec::{write_u32_le, EncodeError};
use crate::mqtt::sensors::EncodablePayload;
use crate::mqtt::topics::TOPIC_SENSOR_DIGITAL_ARMED;

#[derive(Debug, Clone, Copy)]
pub struct ArmedPacket {
    pub timestamp_ms: u32,
    pub value: u8,
}

impl ArmedPacket {
    pub const fn new(timestamp_ms: u32, value: u8) -> Self {
        Self {
            timestamp_ms,
            value,
        }
    }

    pub const fn topic(&self) -> &'static str {
        TOPIC_SENSOR_DIGITAL_ARMED
    }
}

impl EncodablePayload for ArmedPacket {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        if out.len() < 5 {
            return Err(EncodeError::BufferTooSmall);
        }

        write_u32_le(&mut out[..4], self.timestamp_ms)?;
        out[4] = self.value;
        Ok(5)
    }
}
