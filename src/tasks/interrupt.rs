use core::sync::atomic::{AtomicBool, Ordering};

use defmt::{debug, error};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp_hal::{
    gpio::{Input, InputConfig},
    peripherals::GPIO7,
};

use super::power::{PowerHandle, PowerResponse};

// ============================================================================
// STATE
// ============================================================================

static EXT_INTERRUPT_STARTED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SPAWN METHOD
// ============================================================================

pub fn spawn_ext_interrupt_task(
    spawner: &Spawner,
    line: GPIO7<'static>,
    power: PowerHandle,
    other: Option<&'static Signal<CriticalSectionRawMutex, ()>>,
) {
    if EXT_INTERRUPT_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        panic!("external interrupt task already started");
    }

    spawner
        .spawn(ext_interrupt_task(line, power, other))
        .expect("spawn ext interrupt failed");
}

// ============================================================================
// TASK
// ============================================================================

#[embassy_executor::task]
pub async fn ext_interrupt_task(
    line: GPIO7<'static>,
    power: PowerHandle,
    other: Option<&'static Signal<CriticalSectionRawMutex, ()>>,
) {
    let mut pin = Input::new(
        line,
        InputConfig::default().with_pull(esp_hal::gpio::Pull::Up),
    );

    loop {
        pin.wait_for_falling_edge().await;

        match power.check_interrupt().await {
            PowerResponse::Ok => debug!("Power Controller interrupt check ok"),
            PowerResponse::Err(e) => {
                error!("Power Controller interrupt check failed with: {:?}", e)
            }
        }

        if let Some(i) = other {
            i.signal(());
        }
    }
}
