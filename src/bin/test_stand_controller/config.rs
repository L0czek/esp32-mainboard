pub static WIFI_SSID: &str = env!("WIFI_SSID");
pub static WIFI_PASSWORD: &str = env!("WIFI_PASSWORD");
pub static MQTT_HOST: &str = env!("MQTT_HOST");
pub const MQTT_PORT: u16 = 1883;
pub static MQTT_USER: Option<&str> = option_env!("MQTT_USER");
pub static MQTT_PASSWORD: Option<&str> = option_env!("MQTT_PASSWORD");
pub static MQTT_CLIENT_ID: &str = match option_env!("MQTT_CLIENT_ID") {
    Some(id) => id,
    None => "esp32-test-stand",
};
