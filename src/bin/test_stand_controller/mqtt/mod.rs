#![allow(
    dead_code,
    reason = "MQTT data publishing API is staged for upcoming data collection task wiring."
)]

pub mod client;
pub mod codec;
pub mod commands;
pub mod queue;
pub mod sensors;
pub mod topics;

pub use client::mqtt_task;
#[allow(
    unused_imports,
    reason = "Public API is re-exported for the upcoming data collection integration."
)]
pub use queue::{
    publish_armed_sensor, publish_fast_sensors, publish_slow_sensors, publish_temperature_sensor,
    FastSensorsBatch, SlowSensorsBatch,
};
