#![no_std]

pub mod board;
pub mod channel;
pub mod config;
pub mod defmt_ring;
pub mod fire_trigger;
pub mod idle_monitor;
pub mod power;
pub mod signal_light;
pub mod tasks;
pub mod tmp107;
pub mod wifi;

pub use board::I2cType;
