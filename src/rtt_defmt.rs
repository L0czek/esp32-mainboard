//! RTT-only `defmt` sink for binaries without the MQTT log path.
//!
//! `rtt-target`'s own `defmt` feature is disabled package-wide because
//! `test_stand_controller` provides its own tee logger (RTT + MQTT). A
//! `#[defmt::global_logger]` must live in the final binary, so this module
//! only holds the shared implementation; binaries declare the logger with
//! [`rtt_defmt_logger!`](crate::rtt_defmt_logger) and call [`init`] once at boot.

use core::sync::atomic::{AtomicBool, Ordering};

use rtt_target::UpChannel;

const RTT_DEFMT_BUFFER_SIZE: usize = 1024;

static mut CHANNEL: Option<UpChannel> = None;
static TAKEN: AtomicBool = AtomicBool::new(false);
static mut CS_RESTORE: critical_section::RestoreState = critical_section::RestoreState::invalid();
static mut ENCODER: defmt::Encoder = defmt::Encoder::new();

/// Initializes RTT with a single `defmt` up channel.
pub fn init() {
    use rtt_target::ChannelMode::NoBlockSkip;

    let channels = rtt_target::rtt_init! {
        up: {
            0: {
                size: RTT_DEFMT_BUFFER_SIZE,
                mode: NoBlockSkip,
                name: "defmt"
            }
        }
    };

    unsafe {
        CHANNEL = Some(channels.up.0);
    }
}

/// Implements `defmt::Logger::acquire`.
pub fn acquire() {
    let restore = unsafe { critical_section::acquire() };

    if TAKEN.load(Ordering::Relaxed) {
        panic!("defmt logger taken reentrantly");
    }
    TAKEN.store(true, Ordering::Relaxed);

    unsafe {
        CS_RESTORE = restore;
        let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
        encoder.start_frame(do_write);
    }
}

/// Implements `defmt::Logger::release`.
///
/// # Safety
/// Must be paired with a preceding [`acquire`] on the same thread of execution.
pub unsafe fn release() {
    let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
    encoder.end_frame(do_write);
    TAKEN.store(false, Ordering::Relaxed);

    let restore = CS_RESTORE;
    critical_section::release(restore);
}

/// Implements `defmt::Logger::write`.
///
/// # Safety
/// Must only be called between [`acquire`] and [`release`].
pub unsafe fn write(bytes: &[u8]) {
    let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
    encoder.write(bytes, do_write);
}

fn do_write(bytes: &[u8]) {
    unsafe {
        let channel = core::ptr::addr_of_mut!(CHANNEL);
        if let Some(Some(rtt)) = channel.as_mut() {
            rtt.write(bytes);
        }
    }
}

/// Declares the binary's `#[defmt::global_logger]` backed by [`rtt_defmt`](crate::rtt_defmt).
#[macro_export]
macro_rules! rtt_defmt_logger {
    () => {
        #[defmt::global_logger]
        struct RttDefmtLogger;

        unsafe impl defmt::Logger for RttDefmtLogger {
            fn acquire() {
                $crate::rtt_defmt::acquire();
            }

            unsafe fn flush() {}

            unsafe fn release() {
                $crate::rtt_defmt::release();
            }

            unsafe fn write(bytes: &[u8]) {
                $crate::rtt_defmt::write(bytes);
            }
        }
    };
}
