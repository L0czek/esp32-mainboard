//! Shared RTT control block and `defmt` frame encoder.
//!
//! `rtt_init!` defines the `_SEGGER_RTT` control block, which may exist only
//! once per binary, so every binary obtains its RTT channels through [`init`].
//! `rtt-target`'s own `defmt` feature is disabled package-wide because
//! `test_stand_controller` tees frames to MQTT as well. A
//! `#[defmt::global_logger]` must live in the final binary: helper binaries
//! declare an RTT-only one with [`rtt_defmt_logger!`](crate::rtt_defmt_logger),
//! while the controller builds its tee logger on the `*_with` functions.

use core::sync::atomic::{AtomicBool, Ordering};

use rtt_target::UpChannel;

const RTT_DEFMT_BUFFER_SIZE: usize = 1024;
const RTT_PRINT_BUFFER_SIZE: usize = 1024;

/// Byte sink receiving encoded `defmt` frame fragments.
pub type Sink = fn(&[u8]);

static mut CHANNEL: Option<UpChannel> = None;
static TAKEN: AtomicBool = AtomicBool::new(false);
static mut CS_RESTORE: critical_section::RestoreState = critical_section::RestoreState::invalid();
static mut ENCODER: defmt::Encoder = defmt::Encoder::new();

/// Initializes RTT with a `defmt` up channel (0) and a `Terminal` channel (1)
/// used by `rprintln!`.
pub fn init() {
    use rtt_target::ChannelMode::NoBlockSkip;

    let channels = rtt_target::rtt_init! {
        up: {
            0: {
                size: RTT_DEFMT_BUFFER_SIZE,
                mode: NoBlockSkip,
                name: "defmt"
            }
            1: {
                size: RTT_PRINT_BUFFER_SIZE,
                mode: NoBlockSkip,
                name: "Terminal"
            }
        }
    };

    rtt_target::set_print_channel(channels.up.1);
    unsafe {
        CHANNEL = Some(channels.up.0);
    }
}

/// Writes raw bytes to the RTT `defmt` channel; a no-op before [`init`].
pub fn write_rtt(bytes: &[u8]) {
    unsafe {
        let channel = core::ptr::addr_of_mut!(CHANNEL);
        if let Some(Some(rtt)) = channel.as_mut() {
            rtt.write(bytes);
        }
    }
}

/// Implements `defmt::Logger::acquire`, starting a frame into `sink`.
pub fn acquire_with(sink: Sink) {
    let restore = unsafe { critical_section::acquire() };

    if TAKEN.load(Ordering::Relaxed) {
        panic!("defmt logger taken reentrantly");
    }
    TAKEN.store(true, Ordering::Relaxed);

    unsafe {
        CS_RESTORE = restore;
        let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
        encoder.start_frame(sink);
    }
}

/// Implements `defmt::Logger::release`, ending the frame into `sink`.
///
/// # Safety
/// Must be paired with a preceding [`acquire_with`] on the same thread of execution.
pub unsafe fn release_with(sink: Sink) {
    let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
    encoder.end_frame(sink);
    TAKEN.store(false, Ordering::Relaxed);

    let restore = CS_RESTORE;
    critical_section::release(restore);
}

/// Implements `defmt::Logger::write`, encoding `bytes` into `sink`.
///
/// # Safety
/// Must only be called between [`acquire_with`] and [`release_with`].
pub unsafe fn write_with(bytes: &[u8], sink: Sink) {
    let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
    encoder.write(bytes, sink);
}

/// Declares the binary's `#[defmt::global_logger]` writing frames to RTT only.
#[macro_export]
macro_rules! rtt_defmt_logger {
    () => {
        #[defmt::global_logger]
        struct RttDefmtLogger;

        unsafe impl defmt::Logger for RttDefmtLogger {
            fn acquire() {
                $crate::rtt_defmt::acquire_with($crate::rtt_defmt::write_rtt);
            }

            unsafe fn flush() {}

            unsafe fn release() {
                $crate::rtt_defmt::release_with($crate::rtt_defmt::write_rtt);
            }

            unsafe fn write(bytes: &[u8]) {
                $crate::rtt_defmt::write_with(bytes, $crate::rtt_defmt::write_rtt);
            }
        }
    };
}
