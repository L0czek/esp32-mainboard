use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

use defmt::Format;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::{pubsub::PubSubChannel, watch};
use embassy_time::{Ticker, Duration};
use esp_hal::{
    analog::adc::{Adc, AdcCalLine, AdcConfig},
    peripherals::*,
};

use crate::board::{A0Pin, A1Pin, A2Pin, A3Pin, A4Pin, BatVolPin, BoostVolPin};

// ============================================================================
// TYPES
// ============================================================================

const ADC_BUFFER_SIZE: usize = 125;
const ADC_SAMPLE_INTERVAL_MS: u64 = 2;

#[derive(Debug, Format, Clone)]
pub struct AdcState {
    pub battery_voltage: u16,  // in mV
    pub boost_voltage: u16,    // in mV
    pub a0: u16,
    pub a1: u16,
    pub a2: u16,
    pub a3: u16,
    pub a4: u16,
}

#[derive(Debug, Format, Clone)]
pub struct AdcBufferData {
    pub sequence: u32,
    pub battery_voltage: [u16; ADC_BUFFER_SIZE],  // in mV
    pub boost_voltage: [u16; ADC_BUFFER_SIZE],    // in mV
    pub a0: [u16; ADC_BUFFER_SIZE],
    pub a1: [u16; ADC_BUFFER_SIZE],
    pub a2: [u16; ADC_BUFFER_SIZE],
    pub a3: [u16; ADC_BUFFER_SIZE],
    pub a4: [u16; ADC_BUFFER_SIZE],
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
            battery_voltage_calibration: 5624, // ?, calibrated
            boost_voltage_calibration: 13717,  // ?, calibrated
            a0_calibration: 4774,  // 39K / 10K -> 4.9, calibrated
            a1_calibration: 3100,  // 22K / 10K -> 3.2, calibrated
            a2_calibration: 3129,  // 22K / 10K -> 3.2, calibrated
            a3_calibration: 3136,  // 22K / 10K -> 3.2, calibrated
            //a4_calibration: 968,  // 10K / inf -> 1.0, calibrated
            a4_calibration: 14316,  // ^ with another divider on the connector-main-computer board 
        }
    }
}

// ============================================================================
// CHANNELS
// ============================================================================

// ADC state management
static ADC_STATE: watch::Watch<CriticalSectionRawMutex, AdcState, 4> = 
    watch::Watch::new();

pub type AdcStateReceiver = watch::Receiver<'static, CriticalSectionRawMutex, AdcState, 4>;

// ADC buffer data pubsub channel (for full recorded buffers)
static ADC_BUFFER_DATA: PubSubChannel<CriticalSectionRawMutex, AdcBufferData, 2, 4, 1> = 
    PubSubChannel::new();

pub type AdcBufferSubscriber = embassy_sync::pubsub::Subscriber<'static, CriticalSectionRawMutex, AdcBufferData, 2, 4, 1>;

static ADC_STARTED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SPAWN METHOD
// ============================================================================

pub fn spawn_adc_task(
    spawner: &Spawner,
    instance: ADC1<'static>,
    config: AdcConfig<ADC1<'static>>,
    calibration: VoltageMonitorCalibrationConfig,
    bat_pin: BatVolPin,
    boost_pin: BoostVolPin,
    a0_pin: A0Pin,
    a1_pin: A1Pin,
    a2_pin: A2Pin,
    a3_pin: A3Pin,
    a4_pin: A4Pin,
) -> AdcHandle {
    if ADC_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        panic!("ADC task already started");
    }

    spawner
        .spawn(adc_task(
            instance,
            config,
            calibration,
            bat_pin,
            boost_pin,
            a0_pin,
            a1_pin,
            a2_pin,
            a3_pin,
            a4_pin,
        ))
        .expect("spawn ADC task failed");

    AdcHandle { _priv: PhantomData }
}

// ============================================================================
// TASK
// ============================================================================

#[embassy_executor::task]
pub async fn adc_task(
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
        esp_hal::analog::adc::Attenuation::_0dB,
    );
    let mut adc_boost_pin = config.enable_pin_with_cal::<BoostVolPin, AdcCalLine<ADC1<'static>>>(
        boost_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );
    let mut adc_a0_pin = config.enable_pin_with_cal::<A0Pin, AdcCalLine<ADC1<'static>>>(
        a0_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );
    let mut adc_a1_pin = config.enable_pin_with_cal::<A1Pin, AdcCalLine<ADC1<'static>>>(
        a1_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );
    let mut adc_a2_pin = config.enable_pin_with_cal::<A2Pin, AdcCalLine<ADC1<'static>>>(
        a2_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );
    let mut adc_a3_pin = config.enable_pin_with_cal::<A3Pin, AdcCalLine<ADC1<'static>>>(
        a3_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );
    let mut adc_a4_pin = config.enable_pin_with_cal::<A4Pin, AdcCalLine<ADC1<'static>>>(
        a4_pin,
        esp_hal::analog::adc::Attenuation::_0dB,
    );

    let mut adc = Adc::new(instance, config).into_async();

    let adc_state_sender = ADC_STATE.sender();
    let publisher = ADC_BUFFER_DATA.publisher().unwrap();
    let mut sequence: u32 = 0;

    loop {
        let mut buffer = AdcBufferData {
            sequence,
            battery_voltage: [0; ADC_BUFFER_SIZE],
            boost_voltage: [0; ADC_BUFFER_SIZE],
            a0: [0; ADC_BUFFER_SIZE],
            a1: [0; ADC_BUFFER_SIZE],
            a2: [0; ADC_BUFFER_SIZE],
            a3: [0; ADC_BUFFER_SIZE],
            a4: [0; ADC_BUFFER_SIZE],
        };

        // Collect 100 samples at 10ms intervals
        let mut ticker = Ticker::every(Duration::from_millis(ADC_SAMPLE_INTERVAL_MS));
        for i in 0..ADC_BUFFER_SIZE {
            buffer.battery_voltage[i] = ((adc.read_oneshot(&mut adc_bat_pin).await as u32)
                * calibration.battery_voltage_calibration
                / 1000) as u16;
            buffer.boost_voltage[i] = ((adc.read_oneshot(&mut adc_boost_pin).await as u32)
                * calibration.boost_voltage_calibration
                / 1000) as u16;
            buffer.a0[i] = ((adc.read_oneshot(&mut adc_a0_pin).await as u32)
                * calibration.a0_calibration
                / 1000) as u16;
            buffer.a1[i] = ((adc.read_oneshot(&mut adc_a1_pin).await as u32)
                * calibration.a1_calibration
                / 1000) as u16;
            buffer.a2[i] = ((adc.read_oneshot(&mut adc_a2_pin).await as u32)
                * calibration.a2_calibration
                / 1000) as u16;
            buffer.a3[i] = ((adc.read_oneshot(&mut adc_a3_pin).await as u32)
                * calibration.a3_calibration
                / 1000) as u16;
            buffer.a4[i] = ((adc.read_oneshot(&mut adc_a4_pin).await as u32)
                * calibration.a4_calibration
                / 1000) as u16;
            // Maintain sampling interval using ticker
            ticker.next().await;
        }

        // Send the last sample from the buffer as the current state
        let last_idx = ADC_BUFFER_SIZE - 1;
        adc_state_sender.send(AdcState {
            battery_voltage: buffer.battery_voltage[last_idx],
            boost_voltage: buffer.boost_voltage[last_idx],
            a0: buffer.a0[last_idx],
            a1: buffer.a1[last_idx],
            a2: buffer.a2[last_idx],
            a3: buffer.a3[last_idx],
            a4: buffer.a4[last_idx],
        });

        // Publish full buffer data
        publisher.publish_immediate(buffer);
        
        sequence = sequence.wrapping_add(1);
    }
}

// ============================================================================
// HANDLE
// ============================================================================

#[derive(Clone, Copy)]
pub struct AdcHandle {
    _priv: PhantomData<()>,
}

impl AdcHandle {
    pub fn state_receiver(&self) -> Option<AdcStateReceiver> {
        ADC_STATE.receiver()
    }

    pub fn state(&self) -> Option<AdcState> {
        ADC_STATE.try_get()
    }

    pub fn buffer_subscriber(&self) -> Option<AdcBufferSubscriber> {
        ADC_BUFFER_DATA.subscriber().ok()
    }
}
