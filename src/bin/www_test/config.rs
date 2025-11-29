pub static WIFI_SSID: &str = env!("WIFI_SSID");
pub static WIFI_PASSWORD: &str = env!("WIFI_PASSWORD");
pub static AP_SSID: &str = match option_env!("AP_SSID") {
    Some(val) => val,
    None => "ESP32-AP",
};
pub static AP_PASSWORD: &str = match option_env!("AP_PASSWORD") {
    Some(val) => val,
    None => "password123",
};
