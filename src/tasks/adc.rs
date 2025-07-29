use defmt::info;
use esp_hal::{
    analog::adc::{Adc, AdcConfig},
    peripherals::*,
};

#[embassy_executor::task]
pub async fn monitor_voltages(
    instance: ADC1<'static>,
    mut config: AdcConfig<ADC1<'static>>,
    bat_pin: GPIO2<'static>,
    boost_pin: GPIO3<'static>,
) {
    let mut adc_bat_pin = config.enable_pin(bat_pin, esp_hal::analog::adc::Attenuation::_0dB);
    let mut adc_boost_pin = config.enable_pin(boost_pin, esp_hal::analog::adc::Attenuation::_0dB);

    let mut adc = Adc::new(instance, config);

    loop {
        let bat_v = adc.read_oneshot(&mut adc_bat_pin).unwrap();
        let boost_v = adc.read_oneshot(&mut adc_boost_pin).unwrap();

        info!("Battery voltage: {}, Boost voltage: {}", bat_v, boost_v);
    }
}
