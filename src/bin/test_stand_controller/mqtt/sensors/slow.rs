use crate::mqtt::codec::{write_u16_le, write_u32_le, EncodeError};
use crate::mqtt::sensors::EncodablePayload;
use crate::mqtt::topics::{
    TOPIC_SENSOR_ADC_SLOW_BATTERY_COMPUTER, TOPIC_SENSOR_ADC_SLOW_BATTERY_STAND,
    TOPIC_SENSOR_ADC_SLOW_BOOST_VOLTAGE, TOPIC_SENSOR_ADC_SLOW_STARTER_SENSE,
    TOPIC_SENSOR_DIGITAL_ARMED, TOPIC_SENSOR_SERVO,
};

pub const ARMED_MAX_SAMPLES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum SlowAdcChannel {
    BatteryStand,
    BatteryComputer,
    BoostVoltage,
    StarterSense,
}

impl SlowAdcChannel {
    pub const fn topic(self) -> &'static str {
        match self {
            Self::BatteryStand => TOPIC_SENSOR_ADC_SLOW_BATTERY_STAND,
            Self::BatteryComputer => TOPIC_SENSOR_ADC_SLOW_BATTERY_COMPUTER,
            Self::BoostVoltage => TOPIC_SENSOR_ADC_SLOW_BOOST_VOLTAGE,
            Self::StarterSense => TOPIC_SENSOR_ADC_SLOW_STARTER_SENSE,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SlowAdcPacket {
    channel: SlowAdcChannel,
    pub timestamp_ms: u32,
    pub value: u16,
}

impl SlowAdcPacket {
    pub const fn new(channel: SlowAdcChannel, timestamp_ms: u32, value: u16) -> Self {
        Self {
            channel,
            timestamp_ms,
            value,
        }
    }

    pub fn set_channel(&mut self, channel: SlowAdcChannel) {
        self.channel = channel;
    }

    pub const fn channel(&self) -> SlowAdcChannel {
        self.channel
    }

    pub fn topic(&self) -> &'static str {
        self.channel.topic()
    }
}

impl EncodablePayload for SlowAdcPacket {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        if out.len() < 6 {
            return Err(EncodeError::BufferTooSmall);
        }

        write_u32_le(&mut out[..4], self.timestamp_ms)?;
        write_u16_le(&mut out[4..6], self.value)?;
        Ok(6)
    }
}

#[derive(Debug, Clone)]
pub struct ArmedPacket {
    pub first_timestamp_ms: u32,
    pub last_timestamp_ms: u32,
    values: [u8; ARMED_MAX_SAMPLES],
    sample_count: u8,
}

impl ArmedPacket {
    pub fn new(
        first_timestamp_ms: u32,
        last_timestamp_ms: u32,
        values: [u8; ARMED_MAX_SAMPLES],
        sample_count: usize,
    ) -> Result<Self, EncodeError> {
        if sample_count == 0 || sample_count > ARMED_MAX_SAMPLES {
            return Err(EncodeError::TooManySamples);
        }

        Ok(Self {
            first_timestamp_ms,
            last_timestamp_ms,
            values,
            sample_count: sample_count as u8,
        })
    }

    pub fn from_slice(
        first_timestamp_ms: u32,
        last_timestamp_ms: u32,
        values: &[u8],
    ) -> Result<Self, EncodeError> {
        if values.is_empty() || values.len() > ARMED_MAX_SAMPLES {
            return Err(EncodeError::TooManySamples);
        }

        let mut copy = [0u8; ARMED_MAX_SAMPLES];
        copy[..values.len()].copy_from_slice(values);
        Self::new(first_timestamp_ms, last_timestamp_ms, copy, values.len())
    }

    pub fn values(&self) -> &[u8] {
        &self.values[..self.sample_count as usize]
    }

    pub const fn topic(&self) -> &'static str {
        TOPIC_SENSOR_DIGITAL_ARMED
    }
}

impl EncodablePayload for ArmedPacket {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let needed = 8 + self.sample_count as usize;
        if out.len() < needed {
            return Err(EncodeError::BufferTooSmall);
        }

        write_u32_le(&mut out[..4], self.first_timestamp_ms)?;
        write_u32_le(&mut out[4..8], self.last_timestamp_ms)?;
        out[8..needed].copy_from_slice(self.values());

        Ok(needed)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ServoSensorPacket {
    pub timestamp_ms: u32,
    pub value: u16,
}

impl ServoSensorPacket {
    pub const fn new(timestamp_ms: u32, value: u16) -> Self {
        Self {
            timestamp_ms,
            value,
        }
    }

    pub const fn topic(&self) -> &'static str {
        TOPIC_SENSOR_SERVO
    }
}

impl EncodablePayload for ServoSensorPacket {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        if out.len() < 6 {
            return Err(EncodeError::BufferTooSmall);
        }

        write_u32_le(&mut out[..4], self.timestamp_ms)?;
        write_u16_le(&mut out[4..6], self.value)?;
        Ok(6)
    }
}
