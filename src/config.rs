/// Marker inspected by the host device runner before flashing.
pub const fn build_type_marker(configured: bool, release: bool) -> [u8; 23] {
    match (configured, release) {
        (true, true) => *b"FOR_FLASHING_V1!RELEASE",
        (true, false) => *b"FOR_FLASHING_V1!DEBUG..",
        (false, true) => *b"UNCONFIGURED_V1!RELEASE",
        (false, false) => *b"UNCONFIGURED_V1!DEBUG..",
    }
}

/// Only to be called from declared_required_envs
pub const fn build_type_marker_for_config_vars(
    required: &[Option<&'static str>],
    release: bool,
) -> [u8; 23] {
    let mut index = 0;
    while index < required.len() {
        match required[index] {
            Some(value) if !value.is_empty() => {}
            _ => return build_type_marker(false, release),
        }
        index += 1;
    }

    build_type_marker(true, release)
}

/// Declare the environment variables a firmware binary needs for flashing.
/// Call this once at module scope in each binary's `main.rs`.
#[macro_export]
macro_rules! declared_required_envs {
    ($($required:literal),* $(,)?) => {
        #[used]
        #[unsafe(link_section = ".build_type")]
        static BUILD_TYPE_MARKER: [u8; 23] = $crate::config::build_type_marker_for_config_vars(
            &[$(option_env!($required)),*],
            cfg!(release_build),
        );
    };
}

pub static WIFI_SSID: &str = match option_env!("WIFI_SSID") {
    Some(val) => val,
    None => "DEV",
};
pub static WIFI_PASSWORD: &str = match option_env!("WIFI_PASSWORD") {
    Some(val) => val,
    None => "DEV",
};
pub static AP_SSID: &str = match option_env!("AP_SSID") {
    Some(val) => val,
    None => "ESP32-AP",
};
pub static AP_PASSWORD: &str = match option_env!("AP_PASSWORD") {
    Some(val) => val,
    None => "password123",
};
