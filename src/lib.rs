#![feature(impl_trait_in_assoc_type)]
#![no_std]

pub mod board;
pub mod config;
pub mod html;
pub mod power;
pub mod wifi;
pub mod server;

pub use board::Board;
