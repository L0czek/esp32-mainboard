use alloc::format;
use alloc::string::String;
use lazy_static::lazy_static;

pub static BUTTON_DELAY_MS: u64 = 1000;
pub static NTP_SERVER: &str = env!("NTP_SERVER");
pub static MQTT_HOST: &str = env!("MQTT_HOST");
pub const MQTT_PORT: u16 = 1883;
pub static MQTT_USER: Option<&str> = option_env!("MQTT_USER");
pub static MQTT_PASSWORD: Option<&str> = option_env!("MQTT_PASSWORD");
pub static MQTT_CLIENT_ID: &str = match option_env!("MQTT_CLIENT_ID") {
    Some(id) => id,
    None => "esp32-railclock",
};
pub static BATTRY_CALIBRATION: u32 = 5780;

// Battery publish interval (seconds) - configurable
pub static BATTERY_PUBLISH_INTERVAL_SECS: u64 = 120;

lazy_static! {
    pub static ref MQTT_BATTERY_SENSOR_TOPIC: String =
        format!("homeassistant/sensor/{MQTT_CLIENT_ID}/battery");
    pub static ref MQTT_BUTTON_TOPIC: String =
        format!("homeassistant/button/{MQTT_CLIENT_ID}/button/push");
    pub static ref MQTT_BATTERY_SENSOR_CONFIG_TOPIC: String =
        format!("homeassistant/sensor/{MQTT_CLIENT_ID}/battery/config");
    pub static ref MQTT_BUTTON_CONFIG_TOPIC: String =
        format!("homeassistant/button/{MQTT_CLIENT_ID}/button/config");
    pub static ref MQTT_NTP_SYNC_CONFIG_TOPIC: String =
        format!("homeassistant/button/{MQTT_CLIENT_ID}/ntp_sync/config");
    pub static ref MQTT_NTP_SYNC_TOPIC: String =
        format!("homeassistant/button/{MQTT_CLIENT_ID}/button/ntp_sync");
}

lazy_static! {
    /// Discovery JSON payload for the battery sensor entity
    pub static ref MQTT_BATTERY_SENSOR_DISCOVERY: String = {
        let battery_topic = MQTT_BATTERY_SENSOR_TOPIC.as_str();
        format!(
            r#"{{
                "name": "Battery sense",
                "state_topic": "{battery_topic}",
                "unit_of_measurement": "mV",
                "unique_id": "{MQTT_CLIENT_ID}_battery_sensor",
                "device_class": "voltage",
                "device": {{
                    "identifiers": ["{MQTT_CLIENT_ID}-device"],
                    "name": "{MQTT_CLIENT_ID}"
                }}
            }}"#,
        )
    };

    /// Discovery JSON payload for the push button entity
    pub static ref MQTT_PUSH_BUTTON_DISCOVERY: String = {
        let button_topic = MQTT_BUTTON_TOPIC.as_str();
        format!(
            r#"{{
                "name": "Push forward button",
                "command_topic": "{button_topic}",
                "payload_press": "PRESS",
                "unique_id": "{MQTT_CLIENT_ID}_push_button",
                "device": {{
                    "identifiers": ["{MQTT_CLIENT_ID}-device"],
                    "name": "{MQTT_CLIENT_ID}"
                }}
            }}"#,
        )
    };

    /// Discovery JSON payload for the NTP sync button entity
    pub static ref MQTT_NTP_SYNC_DISCOVERY: String = {
        let ntp_topic = MQTT_NTP_SYNC_TOPIC.as_str();
        format!(
            r#"{{
                "name": "NTP sync button",
                "command_topic": "{ntp_topic}",
                "payload_press": "PRESS",
                "unique_id": "{MQTT_CLIENT_ID}_ntp_sync_button",
                "device": {{
                    "identifiers": ["{MQTT_CLIENT_ID}-device"],
                    "name": "{MQTT_CLIENT_ID}"
                }}
            }}"#,
        )
    };

    /// Discovery JSON payload and topics for a shutdown (shipping mode) button
    pub static ref MQTT_SHUTDOWN_CONFIG_TOPIC: String =
        format!("homeassistant/button/{MQTT_CLIENT_ID}/shutdown/config");
    pub static ref MQTT_SHUTDOWN_TOPIC: String =
        format!("homeassistant/button/{MQTT_CLIENT_ID}/button/shutdown");

    pub static ref MQTT_SHUTDOWN_DISCOVERY: String = {
        let shutdown_topic = MQTT_SHUTDOWN_TOPIC.as_str();
        format!(
            r#"{{
                "name": "Shutdown (shipping mode)",
                "command_topic": "{shutdown_topic}",
                "payload_press": "PRESS",
                "unique_id": "{MQTT_CLIENT_ID}_shutdown_button",
                "device": {{
                    "identifiers": ["{MQTT_CLIENT_ID}-device"],
                    "name": "{MQTT_CLIENT_ID}"
                }}
            }}"#,
        )
    };
}
