#![no_std]
#![feature(impl_trait_in_assoc_type)]

pub mod board;
pub mod channel;
pub mod power;
pub mod tasks;

pub use board::I2cType;
