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

/// Parse a TCP port from an optional build-time environment variable.
/// An unset or empty variable yields `default`; anything that is not a decimal
/// number in `1..=65535` fails the build.
pub const fn parse_port_or(value: Option<&str>, default: u16) -> u16 {
    let bytes = match value {
        Some(value) => value.as_bytes(),
        None => return default,
    };
    if bytes.is_empty() {
        return default;
    }

    let mut port: u32 = 0;
    let mut index = 0;
    while index < bytes.len() {
        let digit = bytes[index];
        if !digit.is_ascii_digit() {
            panic!("port must be a decimal number");
        }
        port = port * 10 + (digit - b'0') as u32;
        if port > u16::MAX as u32 {
            panic!("port must be at most 65535");
        }
        index += 1;
    }
    if port == 0 {
        panic!("port must not be 0");
    }

    port as u16
}
