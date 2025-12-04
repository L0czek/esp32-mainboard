use defmt::{debug, error};
use esp_hal::{
    gpio::{Input, InputConfig},
    peripherals::GPIO7,
};

use super::{POWER_CONTROL, PowerResponse};

#[embassy_executor::task]
pub async fn ext_interrupt_task(line: GPIO7<'static>) {
    let mut pin = Input::new(
        line,
        InputConfig::default().with_pull(esp_hal::gpio::Pull::Up),
    );

    loop {
        pin.wait_for_falling_edge().await;

        match POWER_CONTROL
            .transact(super::PowerRequest::CheckInterrupt)
            .await
        {
            PowerResponse::Ok => debug!("Power Controller interrupt check ok"),
            PowerResponse::Err(e) => {
                error!("Power Controller interrupt check failed with: {:?}", e)
            }
        }
    }
}
