use core::str;

use rust_mqtt::types::{MqttString, TopicFilter, TopicName};

pub const TOPIC_SENSOR_ADC_FAST_TENSOMETER: &str = "sensor/adc/fast/tensometer";
pub const TOPIC_SENSOR_ADC_FAST_PRESSURE_TANK: &str = "sensor/adc/fast/pressure/tank";
pub const TOPIC_SENSOR_ADC_FAST_PRESSURE_COMBUSTION: &str = "sensor/adc/fast/pressure/combustion";

pub const TOPIC_SENSOR_ADC_SLOW_BATTERY_STAND: &str = "sensor/adc/slow/battery/stand";
pub const TOPIC_SENSOR_ADC_SLOW_BATTERY_COMPUTER: &str = "sensor/adc/slow/battery/computer";
pub const TOPIC_SENSOR_ADC_SLOW_BOOST_VOLTAGE: &str = "sensor/adc/slow/boost_voltage";
pub const TOPIC_SENSOR_ADC_SLOW_STARTER_SENSE: &str = "sensor/adc/slow/starter_sense";

pub const TOPIC_SENSOR_DIGITAL_ARMED: &str = "sensor/digital/armed";
pub const TOPIC_SENSOR_TEMP_PREFIX: &str = "sensor/temp/";
pub const TOPIC_SENSOR_SERVO: &str = "sensor/servo";

pub const TOPIC_CMD_STATE: &str = "cmd/state";
pub const TOPIC_CMD_SERVO: &str = "cmd/servo";
pub const TOPIC_CMD_SHUTDOWN: &str = "cmd/shutdown";

pub const TOPIC_STATUS_STATE: &str = "status/state";
pub const TOPIC_STATUS_SERVO: &str = "status/servo";
pub const TOPIC_STATUS_CMD: &str = "status/cmd";

pub const COMMAND_TOPICS: [&str; 3] = [TOPIC_CMD_STATE, TOPIC_CMD_SERVO, TOPIC_CMD_SHUTDOWN];

pub const TEMP_TOPIC_BUFFER_LEN: usize = 32;

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum TopicBuildError {
    BufferTooSmall,
    InvalidUtf8,
}

pub fn make_topic_name(topic: &str) -> Option<TopicName<'_>> {
    if topic.is_empty() || topic.contains('#') || topic.contains('+') {
        return None;
    }

    let mqtt_string = MqttString::from_slice(topic).ok()?;
    Some(unsafe { TopicName::new_unchecked(mqtt_string) })
}

pub fn make_topic_filter(topic: &str) -> Option<TopicFilter<'_>> {
    if topic.is_empty() {
        return None;
    }

    let mqtt_string = MqttString::from_slice(topic).ok()?;
    Some(unsafe { TopicFilter::new_unchecked(mqtt_string) })
}

pub fn format_temp_topic(
    sensor_id: u8,
    out: &mut [u8; TEMP_TOPIC_BUFFER_LEN],
) -> Result<&str, TopicBuildError> {
    let prefix = TOPIC_SENSOR_TEMP_PREFIX.as_bytes();
    if prefix.len() >= out.len() {
        return Err(TopicBuildError::BufferTooSmall);
    }

    out[..prefix.len()].copy_from_slice(prefix);
    let written = write_u8_decimal(sensor_id, &mut out[prefix.len()..])?;
    let len = prefix.len() + written;

    str::from_utf8(&out[..len]).map_err(|_| TopicBuildError::InvalidUtf8)
}

fn write_u8_decimal(value: u8, out: &mut [u8]) -> Result<usize, TopicBuildError> {
    if value >= 100 {
        if out.len() < 3 {
            return Err(TopicBuildError::BufferTooSmall);
        }
        out[0] = b'0' + (value / 100);
        out[1] = b'0' + ((value / 10) % 10);
        out[2] = b'0' + (value % 10);
        return Ok(3);
    }

    if value >= 10 {
        if out.len() < 2 {
            return Err(TopicBuildError::BufferTooSmall);
        }
        out[0] = b'0' + (value / 10);
        out[1] = b'0' + (value % 10);
        return Ok(2);
    }

    if out.is_empty() {
        return Err(TopicBuildError::BufferTooSmall);
    }

    out[0] = b'0' + value;
    Ok(1)
}
