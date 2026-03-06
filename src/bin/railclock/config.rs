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
