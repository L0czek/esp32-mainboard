use core::cell::RefCell;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use mainboard::defmt_ring::DefmtRing;
use mainboard::rtt_defmt;

const LOG_RING_CAPACITY: usize = 4096;

static LOG_RING: critical_section::Mutex<RefCell<DefmtRing<LOG_RING_CAPACITY>>> =
    critical_section::Mutex::new(RefCell::new(DefmtRing::new()));
static LOG_AVAILABLE_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Tees every `defmt` frame to RTT and to the MQTT log ring.
#[defmt::global_logger]
struct TeeLogger;

pub fn init() {
    rtt_defmt::init();
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
        rtt_defmt::acquire_with(tee_write);
    }

    unsafe fn flush() {}

    unsafe fn release() {
        rtt_defmt::release_with(tee_write);
    }

    unsafe fn write(bytes: &[u8]) {
        rtt_defmt::write_with(bytes, tee_write);
    }
}

fn tee_write(bytes: &[u8]) {
    rtt_defmt::write_rtt(bytes);
    append_log_bytes(bytes);
}
