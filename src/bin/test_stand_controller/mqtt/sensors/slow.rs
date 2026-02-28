use crate::mqtt::codec::{write_u16_le, write_u32_le, EncodeError};
use crate::mqtt::sensors::EncodablePayload;
use crate::mqtt::topics::{
    TOPIC_SENSOR_ADC_SLOW_BATTERY_COMPUTER, TOPIC_SENSOR_ADC_SLOW_BATTERY_STAND,
    TOPIC_SENSOR_ADC_SLOW_BOOST_VOLTAGE, TOPIC_SENSOR_ADC_SLOW_STARTER_SENSE, TOPIC_SENSOR_SERVO,
};

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
