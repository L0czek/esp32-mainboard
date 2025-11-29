use defmt::Format;
use embassy_time::Timer;
use esp_hal::{
    analog::adc::{Adc, AdcCalLine, AdcConfig},
    peripherals::*,
};

use crate::board::{A0Pin, A1Pin, A2Pin, A3Pin, A4Pin, BatVolPin, BoostVolPin, ADC_STATE};

#[derive(Debug, Format, Clone)]
pub struct AdcState {
    pub battery_voltage: u32,  // in mV
    pub boost_voltage: u32,    // in mV
    pub a0: u32,
    pub a1: u32,
    pub a2: u32,
    pub a3: u32,
    pub a4: u32,
}

pub struct VoltageMonitorCalibrationConfig {
    pub battery_voltage_calibration: u32,
    pub boost_voltage_calibration: u32,
    pub a0_calibration: u32,
    pub a1_calibration: u32,
    pub a2_calibration: u32,
    pub a3_calibration: u32,
    pub a4_calibration: u32,
}

impl Default for VoltageMonitorCalibrationConfig {
    fn default() -> Self {
        Self {
            battery_voltage_calibration: 2000,
            boost_voltage_calibration: 14000,
            a0_calibration: 1000,
            a1_calibration: 1000,
            a2_calibration: 1000,
            a3_calibration: 1000,
            a4_calibration: 1000,
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
    a0_pin: A0Pin,
    a1_pin: A1Pin,
    a2_pin: A2Pin,
    a3_pin: A3Pin,
    a4_pin: A4Pin,
) {
    let mut adc_bat_pin = config.enable_pin_with_cal::<BatVolPin, AdcCalLine<ADC1<'static>>>(
        bat_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_boost_pin = config.enable_pin_with_cal::<BoostVolPin, AdcCalLine<ADC1<'static>>>(
        boost_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_a0_pin = config.enable_pin_with_cal::<A0Pin, AdcCalLine<ADC1<'static>>>(
        a0_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_a1_pin = config.enable_pin_with_cal::<A1Pin, AdcCalLine<ADC1<'static>>>(
        a1_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_a2_pin = config.enable_pin_with_cal::<A2Pin, AdcCalLine<ADC1<'static>>>(
        a2_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_a3_pin = config.enable_pin_with_cal::<A3Pin, AdcCalLine<ADC1<'static>>>(
        a3_pin,
        esp_hal::analog::adc::Attenuation::_11dB,
    );
    let mut adc_a4_pin = config.enable_pin_with_cal::<A4Pin, AdcCalLine<ADC1<'static>>>(
        a4_pin,
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

        let a0 = nb::block!(adc.read_oneshot(&mut adc_a0_pin)).unwrap() as u32
            * calibration.a0_calibration
            / 1000;
        let a1 = nb::block!(adc.read_oneshot(&mut adc_a1_pin)).unwrap() as u32
            * calibration.a1_calibration
            / 1000;
        let a2 = nb::block!(adc.read_oneshot(&mut adc_a2_pin)).unwrap() as u32
            * calibration.a2_calibration
            / 1000;
        let a3 = nb::block!(adc.read_oneshot(&mut adc_a3_pin)).unwrap() as u32
            * calibration.a3_calibration
            / 1000;
        let a4 = nb::block!(adc.read_oneshot(&mut adc_a4_pin)).unwrap() as u32
            * calibration.a4_calibration
            / 1000;

        ADC_STATE.sender().send(AdcState {
            battery_voltage: bat_v,
            boost_voltage: boost_v,
            a0,
            a1,
            a2,
            a3,
            a4,
        });

        Timer::after_secs(1).await;
    }
}
