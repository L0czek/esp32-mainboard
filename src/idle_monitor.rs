use core::sync::atomic::AtomicU32;

use esp_hal::timer::systimer::{SystemTimer, Unit};

/// Default reporting period for idle/busy logs.
pub const DEFAULT_REPORT_INTERVAL_MS: u64 = 5_000;

static TOTAL_IDLE_TICKS: AtomicU32 = AtomicU32::new(0);

/// Idle/busy utilization sample over a measurement window.
#[derive(Clone, Copy, Debug)]
pub struct IdleWindowSample {
    /// Length of the measurement window in hardware timer ticks.
    pub window_ticks: u32,
    /// Time spent in the idle hook in hardware timer ticks.
    pub idle_ticks: u32,
    /// Idle percentage in permille (0..=1000).
    pub idle_permille: u16,
    /// Busy percentage in permille (0..=1000).
    pub busy_permille: u16,
}

/// Tracks successive idle windows.
pub struct IdleWindowTracker {
    window_start_ticks: u64,
    window_start_idle_ticks: u32,
}

impl IdleWindowTracker {
    /// Creates a tracker starting from the current timestamp and idle counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            window_start_ticks: monotonic_ticks(),
            window_start_idle_ticks: total_idle_ticks(),
        }
    }

    /// Captures utilization since the previous sample and resets the window start.
    #[must_use]
    pub fn sample_and_reset(&mut self) -> IdleWindowSample {
        let end_ticks = monotonic_ticks();
        let end_idle_ticks = total_idle_ticks();

        // We assume sampling is performed max every 20s. With 180Mhz clock, that gives us 180e6 * 20 = 3.6e9 < 4e9
        let window_ticks = end_ticks.wrapping_sub(self.window_start_ticks) as u32;
        let idle_ticks = end_idle_ticks.wrapping_sub(self.window_start_idle_ticks);

        self.window_start_ticks = end_ticks;
        self.window_start_idle_ticks = end_idle_ticks;

        let idle_permille = utilization_permille(idle_ticks, window_ticks);
        let busy_permille = 1_000u16.saturating_sub(idle_permille);

        IdleWindowSample {
            window_ticks,
            idle_ticks,
            idle_permille,
            busy_permille,
        }
    }
}

impl Default for IdleWindowTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Custom idle hook for `esp_rtos::start_with_idle_hook`.
///
/// The hook blocks on WFI/WAITI and accumulates elapsed hardware timer ticks.
pub extern "C" fn idle_hook() -> ! {
    loop {
        critical_section::with(|_cs| {
            // Keep timing + accumulation atomic with respect to scheduler preemption.
            let idle_start_ticks = monotonic_ticks();
            wait_for_interrupt();
            let idle_end_ticks = monotonic_ticks();
            // we assume an interrupt won't take more than 2^32 ticks (~22s at 180MHz), so we won't miss any idle time due to wrapping
            let idle_delta_ticks = idle_end_ticks.wrapping_sub(idle_start_ticks) as u32;
            let total_idle_ticks = TOTAL_IDLE_TICKS.load(core::sync::atomic::Ordering::Relaxed);
            TOTAL_IDLE_TICKS.store(
                total_idle_ticks.wrapping_add(idle_delta_ticks),
                core::sync::atomic::Ordering::Relaxed,
            );
        });
    }
}

/// Returns cumulative idle ticks since boot.
#[must_use]
pub fn total_idle_ticks() -> u32 {
    TOTAL_IDLE_TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

/// Returns the current monotonic timestamp in hardware timer ticks.
#[must_use]
pub fn monotonic_ticks() -> u64 {
    SystemTimer::unit_value(Unit::Unit0)
}

/// Returns hardware timer frequency in ticks per second.
#[must_use]
pub fn timer_ticks_per_mili() -> u64 {
    SystemTimer::ticks_per_second() / 1000
}

/// Converts hardware timer ticks to milliseconds.
#[must_use]
pub fn ticks_to_millis(ticks: u32) -> u32 {
    let numerator = u64::from(ticks);
    let denominator = timer_ticks_per_mili();
    (numerator / denominator) as u32
}

fn utilization_permille(part: u32, whole: u32) -> u16 {
    if whole == 0 {
        return 0;
    }

    let ratio = u64::from(part).saturating_mul(1_000) / u64::from(whole);
    u16::try_from(ratio.min(u64::from(1_000u16))).unwrap_or(1_000)
}

#[inline]
fn wait_for_interrupt() {
    #[cfg(target_arch = "riscv32")]
    {
        // SAFETY: WFI is the intended idle instruction on ESP32-C6 (RISC-V).
        unsafe { core::arch::asm!("wfi") };
    }

    #[cfg(target_arch = "xtensa")]
    {
        // SAFETY: WAITI is the intended idle instruction on Xtensa targets.
        unsafe { core::arch::asm!("waiti 0") };
    }
}
