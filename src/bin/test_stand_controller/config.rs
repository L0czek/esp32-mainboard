pub static WIFI_SSID: &str = env!("WIFI_SSID");
pub static WIFI_PASSWORD: &str = env!("WIFI_PASSWORD");
pub static MQTT_HOST: &str = env!("MQTT_HOST");
pub static MQTT_USER: Option<&str> = option_env!("MQTT_USER");
pub static MQTT_PASSWORD: Option<&str> = option_env!("MQTT_PASSWORD");
