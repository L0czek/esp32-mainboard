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

/// Number of ADC one-shot reads averaged into one published sensor sample.
///
/// Each blocking one-shot read costs ~52 us on the ESP32-C6, and the three fast
/// channels are sampled every millisecond, so every increment adds ~16% CPU load
/// in a release build. Measured: N=1 47%, N=3 80% busy; N=4 saturates the CPU
/// and drops MQTT batches. Debug builds have no headroom for oversampling.
pub const ADC_OVERSAMPLING_SAMPLES: usize = 3;

pub const BLACKBOX_BAUD_RATE: u32 = 3_000_000;

// =============================================
//                    FIRE
// =============================================

pub const FIRE_TRIGGER_BYTE: u8 = 0x00;
pub const FIRE_COUNTDOWN_DURATION_MS: u64 = 10_000;
