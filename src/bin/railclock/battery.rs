use core::marker::PhantomData;

use defmt::Format;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch};
use embassy_time::{Duration, Ticker};
use esp_hal::{
    analog::adc::{Adc, AdcCalLine, AdcConfig},
    peripherals::ADC1,
};

use mainboard::board::BatVolPin;

// Simple battery monitor: publishes latest battery voltage (in mV) to a watch channel.

static BATTERY_STATE: watch::Watch<CriticalSectionRawMutex, u16, 4> = watch::Watch::new();

#[derive(Clone, Copy, Debug, Format)]
pub struct BatteryHandle {
    _priv: PhantomData<()>,
}

impl BatteryHandle {
    pub fn state_receiver(
        &self,
    ) -> Option<watch::Receiver<'static, CriticalSectionRawMutex, u16, 4>> {
        BATTERY_STATE.receiver()
    }

    pub fn state(&self) -> Option<u16> {
        BATTERY_STATE.try_get()
    }
}

pub struct BatteryCalibration {
    pub battery_voltage_calibration: u32,
}

impl Default for BatteryCalibration {
    fn default() -> Self {
        Self {
            battery_voltage_calibration: 5624,
        }
    }
}

pub fn spawn_battery_task(
    spawner: &Spawner,
    instance: ADC1<'static>,
    config: AdcConfig<ADC1<'static>>,
    calibration: BatteryCalibration,
    bat_pin: BatVolPin,
    publish_interval_secs: Option<u64>,
    publish_topic: Option<&'static str>,
) -> BatteryHandle {
    spawner
        .spawn(battery_task(
            instance,
            config,
            calibration,
            bat_pin,
            publish_interval_secs,
            publish_topic,
        ))
        .expect("spawn battery task failed");
    BatteryHandle { _priv: PhantomData }
}

#[embassy_executor::task]
async fn battery_task(
    instance: ADC1<'static>,
    mut config: AdcConfig<ADC1<'static>>,
    calibration: BatteryCalibration,
    bat_pin: BatVolPin,
    publish_interval_secs: Option<u64>,
    publish_topic: Option<&'static str>,
) {
    let mut adc_bat_pin = config.enable_pin_with_cal::<BatVolPin, AdcCalLine<ADC1<'static>>>(
        bat_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );

    let mut adc = Adc::new(instance, config).into_async();

    let sender = BATTERY_STATE.sender();

    // Sample interval controlled by publish_interval_secs (if provided)
    let mut ticker = Ticker::every(Duration::from_secs(publish_interval_secs.unwrap_or(600)));

    loop {
        // take a small burst of readings and average
        let mut sum: u32 = 0;
        const SAMPLES: usize = 8;
        for _ in 0..SAMPLES {
            let raw = adc.read_oneshot(&mut adc_bat_pin).await as u32;
            #[allow(non_snake_case)]
            let mV = raw * calibration.battery_voltage_calibration / 1000;
            sum += mV;
        }
        #[allow(non_snake_case)]
        let avg_mV = (sum / (SAMPLES as u32)) as u16;

        sender.send(avg_mV);

        // Publish via MQTT if configured
        if let Some(topic) = publish_topic {
            // Format as volts with two decimals
            let payload = alloc::format!("{}", avg_mV);
            let _ = crate::mqtt_queue::mqtt_publish(topic, &payload, true);
        }

        // wait until next sample/publish
        ticker.next().await;
    }
}
