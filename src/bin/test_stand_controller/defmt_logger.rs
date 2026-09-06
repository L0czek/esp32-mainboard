use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use mainboard::defmt_ring::DefmtRing;
use rtt_target::UpChannel;

const RTT_DEFMT_BUFFER_SIZE: usize = 1024;
const RTT_PRINT_BUFFER_SIZE: usize = 1024;
const LOG_RING_CAPACITY: usize = 4096;

static mut RTT_CHANNEL: Option<UpChannel> = None;
static TAKEN: AtomicBool = AtomicBool::new(false);
static mut CS_RESTORE: critical_section::RestoreState = critical_section::RestoreState::invalid();
static mut ENCODER: defmt::Encoder = defmt::Encoder::new();
static LOG_RING: critical_section::Mutex<RefCell<DefmtRing<LOG_RING_CAPACITY>>> =
    critical_section::Mutex::new(RefCell::new(DefmtRing::new()));
static LOG_AVAILABLE_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[defmt::global_logger]
struct TeeLogger;

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
        RTT_CHANNEL = Some(channels.up.0);
    }
}

#[embassy_executor::task]
pub async fn drain_task() {
    let mut payload = [0u8; crate::mqtt::queue::DEFMT_LOG_CHUNK_SIZE];

    loop {
        let len = take_log_chunk(&mut payload);
        if len == 0 {
            LOG_AVAILABLE_SIGNAL.wait().await;
            continue;
        }

        if let Err(error) = crate::mqtt::queue::publish_defmt_log_chunk(&payload[..len]) {
            rtt_target::rprintln!("defmt MQTT log publish failed: {:?}", error);
        }
    }
}

fn take_log_chunk(out: &mut [u8]) -> usize {
    critical_section::with(|cs| LOG_RING.borrow_ref_mut(cs).pop_into(out))
}

fn append_log_bytes(bytes: &[u8]) {
    let mut wrote_bytes = false;
    critical_section::with(|cs| {
        wrote_bytes = LOG_RING.borrow_ref_mut(cs).push_slice(bytes) != 0;
    });

    if wrote_bytes {
        LOG_AVAILABLE_SIGNAL.signal(());
    }
}

unsafe impl defmt::Logger for TeeLogger {
    fn acquire() {
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

    unsafe fn flush() {}

    unsafe fn release() {
        let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
        encoder.end_frame(do_write);
        TAKEN.store(false, Ordering::Relaxed);

        let restore = CS_RESTORE;
        critical_section::release(restore);
    }

    unsafe fn write(bytes: &[u8]) {
        let encoder = &mut *core::ptr::addr_of_mut!(ENCODER);
        encoder.write(bytes, do_write);
    }
}

fn do_write(bytes: &[u8]) {
    unsafe {
        let channel = core::ptr::addr_of_mut!(RTT_CHANNEL);
        if let Some(Some(rtt)) = channel.as_mut() {
            rtt.write(bytes);
        }
    }

    append_log_bytes(bytes);
}
