use defmt::warn;
use embassy_time::{Instant, Timer};
use esp_hal::analog::adc::{
    Adc, AdcCalBasic, AdcCalScheme, AdcChannel, AdcConfig, AdcPin, Attenuation,
};
use esp_hal::peripherals::ADC1;
use esp_hal::Blocking;

use crate::mqtt::sensors::fast::{FastAdcChannel, FastAdcPacket};
use crate::mqtt::sensors::slow::{SlowAdcChannel, SlowAdcPacket};
use crate::mqtt::{publish_fast_sensors, publish_slow_sensors, FastSensorsBatch, SlowSensorsBatch};
use mainboard::board::{A0Pin, A1Pin, A2Pin, A3Pin, A4Pin, BatVolPin, BoostVolPin};

const FAST_BATCH_SAMPLES: usize = 100;

pub struct SensorCollectionIo {
    pub adc: ADC1<'static>,
    pub tensometer: A0Pin,
    pub pressure_tank: A1Pin,
    pub pressure_combustion: A2Pin,
    pub starter_sense: A3Pin,
    pub battery_stand: A4Pin,
    pub battery_computer: BatVolPin,
    pub boost_voltage: BoostVolPin,
}

struct SensorCollectionState {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    tensometer: AdcPin<A0Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    pressure_tank: AdcPin<A1Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    pressure_combustion: AdcPin<A2Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    starter_sense: AdcPin<A3Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    battery_stand: AdcPin<A4Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    battery_computer: AdcPin<BatVolPin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    boost_voltage: AdcPin<BoostVolPin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
}

impl SensorCollectionState {
    fn new(io: SensorCollectionIo) -> Self {
        let mut config = AdcConfig::new();

        let tensometer = config.enable_pin_with_cal::<A0Pin, AdcCalBasic<ADC1<'static>>>(
            io.tensometer,
            Attenuation::_0dB,
        );
        let pressure_tank = config.enable_pin_with_cal::<A1Pin, AdcCalBasic<ADC1<'static>>>(
            io.pressure_tank,
            Attenuation::_0dB,
        );
        let pressure_combustion = config.enable_pin_with_cal::<A2Pin, AdcCalBasic<ADC1<'static>>>(
            io.pressure_combustion,
            Attenuation::_0dB,
        );
        let starter_sense = config.enable_pin_with_cal::<A3Pin, AdcCalBasic<ADC1<'static>>>(
            io.starter_sense,
            Attenuation::_0dB,
        );
        let battery_stand = config.enable_pin_with_cal::<A4Pin, AdcCalBasic<ADC1<'static>>>(
            io.battery_stand,
            Attenuation::_0dB,
        );
        let battery_computer = config.enable_pin_with_cal::<BatVolPin, AdcCalBasic<ADC1<'static>>>(
            io.battery_computer,
            Attenuation::_0dB,
        );
        let boost_voltage = config.enable_pin_with_cal::<BoostVolPin, AdcCalBasic<ADC1<'static>>>(
            io.boost_voltage,
            Attenuation::_0dB,
        );

        let adc = Adc::new(io.adc, config);

        Self {
            adc,
            tensometer,
            pressure_tank,
            pressure_combustion,
            starter_sense,
            battery_stand,
            battery_computer,
            boost_voltage,
        }
    }
}

#[embassy_executor::task]
pub async fn sensor_collection_task(io: SensorCollectionIo) {
    let mut state = SensorCollectionState::new(io);

    loop {
        collect_and_publish_fast(&mut state).await;
        collect_and_publish_slow(&mut state);
    }
}

async fn collect_and_publish_fast(state: &mut SensorCollectionState) {
    let mut tensometer = [0u16; FAST_BATCH_SAMPLES];
    let mut pressure_tank = [0u16; FAST_BATCH_SAMPLES];
    let mut pressure_combustion = [0u16; FAST_BATCH_SAMPLES];

    let mut first_timestamp_ms = 0u32;
    let mut last_timestamp_ms = 0u32;

    for index in 0..FAST_BATCH_SAMPLES {
        let timestamp_ms = timestamp_ms();
        if index == 0 {
            first_timestamp_ms = timestamp_ms;
        }
        last_timestamp_ms = timestamp_ms;

        tensometer[index] = read_adc_raw(&mut state.adc, &mut state.tensometer);
        pressure_tank[index] = read_adc_raw(&mut state.adc, &mut state.pressure_tank);
        pressure_combustion[index] = read_adc_raw(&mut state.adc, &mut state.pressure_combustion);

        if index + 1 < FAST_BATCH_SAMPLES {
            Timer::after_millis(1).await;
        }
    }

    let tensometer_packet = FastAdcPacket::from_slice(
        FastAdcChannel::Tensometer,
        first_timestamp_ms,
        last_timestamp_ms,
        &tensometer,
    )
    .expect("fast packet validation failed for tensometer");

    let tank_packet = FastAdcPacket::from_slice(
        FastAdcChannel::PressureTank,
        first_timestamp_ms,
        last_timestamp_ms,
        &pressure_tank,
    )
    .expect("fast packet validation failed for tank pressure");

    let combustion_packet = FastAdcPacket::from_slice(
        FastAdcChannel::PressureCombustion,
        first_timestamp_ms,
        last_timestamp_ms,
        &pressure_combustion,
    )
    .expect("fast packet validation failed for combustion pressure");

    let batch = FastSensorsBatch {
        tensometer: Some(tensometer_packet),
        tank_pressure: Some(tank_packet),
        combustion_pressure: Some(combustion_packet),
    };

    if publish_fast_sensors(batch).is_err() {
        warn!("Dropping fast sensors batch: outbound queue full");
    }
}

fn collect_and_publish_slow(state: &mut SensorCollectionState) {
    let battery_stand = SlowAdcPacket::new(
        SlowAdcChannel::BatteryStand,
        timestamp_ms(),
        read_adc_raw(&mut state.adc, &mut state.battery_stand),
    );

    let battery_computer = SlowAdcPacket::new(
        SlowAdcChannel::BatteryComputer,
        timestamp_ms(),
        read_adc_raw(&mut state.adc, &mut state.battery_computer),
    );

    let boost_voltage = SlowAdcPacket::new(
        SlowAdcChannel::BoostVoltage,
        timestamp_ms(),
        read_adc_raw(&mut state.adc, &mut state.boost_voltage),
    );

    let starter_sense = SlowAdcPacket::new(
        SlowAdcChannel::StarterSense,
        timestamp_ms(),
        read_adc_raw(&mut state.adc, &mut state.starter_sense),
    );

    let batch = SlowSensorsBatch {
        battery_stand: Some(battery_stand),
        battery_computer: Some(battery_computer),
        boost_voltage: Some(boost_voltage),
        starter_sense: Some(starter_sense),
        servo: None,
    };

    if publish_slow_sensors(batch).is_err() {
        warn!("Dropping slow sensors batch: outbound queue full");
    }
}

fn read_adc_raw<PIN, CS>(
    adc: &mut Adc<'static, ADC1<'static>, Blocking>,
    pin: &mut AdcPin<PIN, ADC1<'static>, CS>,
) -> u16
where
    PIN: AdcChannel,
    CS: AdcCalScheme<ADC1<'static>>,
{
    nb::block!(adc.read_oneshot(pin)).expect("ADC oneshot read failed")
}

fn timestamp_ms() -> u32 {
    Instant::now().as_millis() as u32
}
