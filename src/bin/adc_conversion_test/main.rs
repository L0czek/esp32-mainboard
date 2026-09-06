#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use esp_hal::analog::adc::{
    Adc, AdcCalBasic, AdcCalScheme, AdcChannel, AdcConfig, AdcPin, Attenuation,
};
use esp_hal::clock::CpuClock;
use esp_hal::peripherals::ADC1;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{Async, Blocking};
use mainboard::board::{A0Pin, Board};
use mainboard::create_board;
use panic_rtt_target as _;

const CONVERSION_COUNT: usize = 1_000;
const BENCHMARK_INTERVAL_MS: u64 = 1_000;

type A0AdcPin = AdcPin<A0Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>;

// This creates the app descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

mainboard::rtt_defmt_logger!();

#[derive(defmt::Format)]
struct BenchmarkResult {
    elapsed_us: u64,
    average_ns: u64,
    min_raw: u16,
    max_raw: u16,
    last_raw: u16,
}

impl BenchmarkResult {
    fn new(elapsed_us: u64, samples: &[u16]) -> Self {
        let mut min_raw = u16::MAX;
        let mut max_raw = u16::MIN;
        for &sample in samples {
            min_raw = min_raw.min(sample);
            max_raw = max_raw.max(sample);
        }
        Self {
            elapsed_us,
            average_ns: (elapsed_us * 1_000) / samples.len() as u64,
            min_raw,
            max_raw,
            last_raw: samples[samples.len() - 1],
        }
    }
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    mainboard::rtt_defmt::init();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 32768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    info!("Embassy initialized for ADC conversion benchmark");

    let board = create_board!(peripherals);

    let mut adc_config = AdcConfig::new();
    let mut test_pin = adc_config
        .enable_pin_with_cal::<A0Pin, AdcCalBasic<ADC1<'static>>>(board.A0, Attenuation::_0dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_config);

    loop {
        let blocking = benchmark_blocking(&mut adc, &mut test_pin);
        info!(
            "ADC blocking A0: {} conversions {}",
            CONVERSION_COUNT, blocking
        );

        let mut async_adc = adc.into_async();
        let asynchronous = benchmark_async(&mut async_adc, &mut test_pin).await;
        info!(
            "ADC async    A0: {} conversions {}",
            CONVERSION_COUNT, asynchronous
        );
        adc = async_adc.into_blocking();

        Timer::after_millis(BENCHMARK_INTERVAL_MS).await;
    }
}

fn benchmark_blocking(
    adc: &mut Adc<'static, ADC1<'static>, Blocking>,
    pin: &mut A0AdcPin,
) -> BenchmarkResult {
    let mut samples = [0u16; CONVERSION_COUNT];
    let _warmup = read_adc_raw(adc, pin);

    let start = Instant::now();
    for sample in samples.iter_mut() {
        *sample = read_adc_raw(adc, pin);
    }
    let elapsed_us = Instant::now().duration_since(start).as_micros();

    BenchmarkResult::new(elapsed_us, &samples)
}

async fn benchmark_async(
    adc: &mut Adc<'static, ADC1<'static>, Async>,
    pin: &mut A0AdcPin,
) -> BenchmarkResult {
    let mut samples = [0u16; CONVERSION_COUNT];
    let _warmup = adc.read_oneshot(pin).await;

    let start = Instant::now();
    for sample in samples.iter_mut() {
        *sample = adc.read_oneshot(pin).await;
    }
    let elapsed_us = Instant::now().duration_since(start).as_micros();

    BenchmarkResult::new(elapsed_us, &samples)
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
