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
use esp_hal::Blocking;
use mainboard::board::{A0Pin, Board};
use mainboard::create_board;
use panic_rtt_target as _;

const CONVERSION_COUNT: usize = 1_000;
const BENCHMARK_INTERVAL_MS: u64 = 1_000;

// This creates the app descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

mainboard::rtt_defmt_logger!();

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
        let (elapsed_us, last_raw) = benchmark_single_pin(&mut adc, &mut test_pin);
        let average_ns = (elapsed_us * 1_000) / CONVERSION_COUNT as u64;

        info!(
            "ADC benchmark A0: {} conversions in {} us (avg {} ns/conv), last_raw={}",
            CONVERSION_COUNT, elapsed_us, average_ns, last_raw
        );

        Timer::after_millis(BENCHMARK_INTERVAL_MS).await;
    }
}

fn benchmark_single_pin(
    adc: &mut Adc<'static, ADC1<'static>, Blocking>,
    pin: &mut AdcPin<A0Pin, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
) -> (u64, u16) {
    let warmup_sample = read_adc_raw(adc, pin);
    let start = Instant::now();
    let mut last_sample = warmup_sample;

    for _ in 0..CONVERSION_COUNT {
        last_sample = read_adc_raw(adc, pin);
    }

    let elapsed_us = Instant::now().duration_since(start).as_micros();
    (elapsed_us, last_sample)
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
