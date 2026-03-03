// =============================================
//                    MQTT
// =============================================

pub static MQTT_HOST: &str = env!("MQTT_HOST");
pub const MQTT_PORT: u16 = 1883;
pub static MQTT_USER: Option<&str> = option_env!("MQTT_USER");
pub static MQTT_PASSWORD: Option<&str> = option_env!("MQTT_PASSWORD");
pub static MQTT_CLIENT_ID: &str = match option_env!("MQTT_CLIENT_ID") {
    Some(id) => id,
    None => "esp32-test-stand",
};

// =============================================
//              Temperature (TMP107)
// =============================================

pub const TEMP_COLLECTION_INTERVAL_MS: u64 = 50;
/// Number of temperature readings to collect into one MQTT packet
pub const TEMP_BATCH_SIZE: usize = 20;
pub const TEMP_UART_BOUDRATE: u32 = 115200;

// =============================================
//                    SERVO
// =============================================

// Servo pulse width range (MCPWM ticks mapping physical 0-180 degrees)
pub const SERVO_MIN_PULSE_TICKS: u16 = 500;
pub const SERVO_MAX_PULSE_TICKS: u16 = 2500;

// Operational positions (degrees within the 0-1800 range)
pub const SERVO_OPEN_DEGREES: u16 = 975;
pub const SERVO_CLOSED_DEGREES: u16 = 1800;

// Time for full 0-180 degree travel
pub const SERVO_FULL_RANGE_MS: u64 = 5000;
