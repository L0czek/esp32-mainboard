use defmt::info;
use embassy_time::Timer;
use esp_hal::{
    analog::adc::{Adc, AdcCalLine, AdcConfig},
    peripherals::*,
};

use crate::board::{BatVolPin, BoostVolPin};

pub struct VoltageMonitorCalibrationConfig {
    pub battery_voltage_calibration: u32,
    pub boost_voltage_calibration: u32,
}

impl Default for VoltageMonitorCalibrationConfig {
    fn default() -> Self {
        Self {
            battery_voltage_calibration: 2000,
            boost_voltage_calibration: 14000,
        }
    }
}

#[embassy_executor::task]
pub async fn monitor_voltages(
    instance: ADC1<'static>,
    mut config: AdcConfig<ADC1<'static>>,
    calibration: VoltageMonitorCalibrationConfig,
    bat_pin: BatVolPin,
    boost_pin: BoostVolPin,
) {
    let mut adc_bat_pin = config.enable_pin_with_cal::<BatVolPin, AdcCalLine<ADC1<'static>>>(
        bat_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_boost_pin = config.enable_pin_with_cal::<BoostVolPin, AdcCalLine<ADC1<'static>>>(
        boost_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );

    let mut adc = Adc::new(instance, config);

    loop {
        let bat_v = nb::block!(adc.read_oneshot(&mut adc_bat_pin)).unwrap() as u32
            * calibration.battery_voltage_calibration
            / 1000;
        let boost_v = nb::block!(adc.read_oneshot(&mut adc_boost_pin)).unwrap() as u32
            * calibration.boost_voltage_calibration
            / 1000;

        info!("Battery voltage: {}, Boost voltage: {}", bat_v, boost_v);

        Timer::after_secs(10).await;
    }
}
