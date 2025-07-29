#![feature(impl_trait_in_assoc_type)]
#![no_std]

pub mod board;
pub mod config;
pub mod html;
pub mod power;
pub mod server;
pub mod simple_output;
pub mod wifi;

pub use board::Board;
