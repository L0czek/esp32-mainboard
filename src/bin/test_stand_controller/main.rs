#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod camera_shutter;
mod config;
mod mqtt;
mod sensor_collection;
mod sequencer;
mod servo;
mod temperature_collection;

use mainboard::board::{acquire_i2c_bus, init_i2c_bus, Board};
use mainboard::create_board;
use mainboard::power::PowerControllerIO;
use mainboard::tasks::{
    spawn_ext_interrupt_task, spawn_power_controller, PowerResponse, PowerStateReceiver,
};
use mainboard::wifi::{initialize_wifi_sta, WifiResourceSta};

use defmt::info;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp_hal::clock::CpuClock;
use esp_hal::rtc_cntl::Rtc;
use esp_hal::timer::timg::TimerGroup;
use panic_rtt_target as _;
use static_cell::StaticCell;

// StaticCell for WiFi controller
static ESP_RADIO_INIT: StaticCell<esp_radio::Controller<'static>> = StaticCell::new();
// StaticCell for WiFi resources (needed for mqtt_task which requires 'static lifetime)
static WIFI_RESOURCES: StaticCell<WifiResourceSta> = StaticCell::new();
static SHUTDOWN_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "Main setup uses large stack objects during startup and shutdown wiring."
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    // Initialize RTT for logging
    rtt_target::rtt_init_defmt!();

    // Configure and initialize hardware
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // Initialize heap allocator
    // Not #[esp_hal::ram(reclaimed)] because its too small XD
    // We use all we can afford to hopefully have enough for serde XD
    esp_alloc::heap_allocator!(size: 65536 * 2);

    // Initialize esp_rtos
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    info!("Embassy initialized!");

    let board = create_board!(peripherals);

    init_i2c_bus(peripherals.I2C0, board.Sda, board.Scl).expect("Failed to initialize I2C bus");

    // Initialize RNG for WiFi
    let mut rng = esp_hal::rng::Rng::new();

    // Initialize esp-radio controller
    let radio_init =
        ESP_RADIO_INIT.init(esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller"));

    let sensor_collection_io = sensor_collection::SensorCollectionIo {
        adc: peripherals.ADC1,
        tensometer: board.A0,
        pressure_tank: board.A1,
        pressure_combustion: board.A2,
        starter_sense: board.A3,
        battery_stand: board.A4,
        battery_computer: board.BatVol,
        boost_voltage: board.BoostVol,
    };

    let power_config = Default::default();
    let power_io = PowerControllerIO {
        charger_i2c: acquire_i2c_bus(),
        pcf8574_i2c: acquire_i2c_bus(),
        boost_converter_enable: board.BoostEn,
    };
    let power = spawn_power_controller(&spawner, power_config, power_io);

    match power.set_boost_converter(true).await {
        PowerResponse::Ok => info!("Boost converter enabled"),
        PowerResponse::Err(e) => {
            info!("Failed to enable boost converter: {:?}", e)
        }
    }

    let power_receiver = power
        .state_receiver()
        .expect("Failed to get power state receiver");
    spawner
        .spawn(log_power_state_changes_task(power_receiver))
        .expect("Failed to spawn log_power_state_changes_task");

    spawn_ext_interrupt_task(&spawner, board.GlobalInt, power, None);

    // Initialize WiFi in STA mode
    info!("Initializing WiFi...");
    let wifi_resources = initialize_wifi_sta(spawner, radio_init, peripherals.WIFI, &mut rng).await;
    info!("WiFi initialized!");

    // Store wifi resources in static cell for mqtt_task
    let wifi_resources = WIFI_RESOURCES.init(wifi_resources);

    // Spawn MQTT task
    spawner
        .spawn(mqtt::mqtt_task(wifi_resources, &SHUTDOWN_SIGNAL))
        .expect("Failed to spawn mqtt_task");
    info!("MQTT task spawned");

    spawner
        .spawn(sensor_collection::sensor_collection_task(
            sensor_collection_io,
        ))
        .expect("Failed to spawn sensor_collection_task");
    info!("Sensor collection task spawned");

    let temp_io = temperature_collection::TemperatureCollectionIo {
        uart: peripherals.UART0,
        tx_pin: board.U0Tx,
        rx_pin: board.U0Rx,
        dir_pin: board.D0,
    };

    spawner
        .spawn(temperature_collection::temperature_collection_task(temp_io))
        .expect("Failed to spawn temperature_collection_task");
    info!("Temperature collection task spawned");

    spawner
        .spawn(servo::servo_controller_task(peripherals.MCPWM0, board.D1))
        .expect("Failed to spawn servo_controller_task");
    info!("Servo controller task spawned");

    let shutter_pin = esp_hal::gpio::Output::new(
        board.D4,
        esp_hal::gpio::Level::Low,
        esp_hal::gpio::OutputConfig::default(),
    );
    spawner
        .spawn(camera_shutter::camera_shutter_task(shutter_pin))
        .expect("Failed to spawn camera_shutter_task");
    info!("Camera shutter task spawned");

    let armed_pin = esp_hal::gpio::Input::new(
        board.D2,
        esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::Up),
    );
    sequencer::init_armed_state(&armed_pin);
    let signal_light_i2c = acquire_i2c_bus();
    let fire_trigger_i2c = acquire_i2c_bus();
    spawner
        .spawn(sequencer::fire_sequencer_task(fire_trigger_i2c))
        .expect("Failed to spawn fire_sequencer_task");
    spawner
        .spawn(sequencer::state_sequencer_task(armed_pin, signal_light_i2c))
        .expect("Failed to spawn state_sequencer_task");
    info!("State sequencer task spawned");

    SHUTDOWN_SIGNAL.wait().await;
    info!("Shutdown signal received");

    // Perform shutdown sequence
    info!("Executing shutdown sequence: disable boost, set charger to Charging, float GPIOs");
    match power.set_boost_converter(false).await {
        PowerResponse::Ok => info!("Boost converter disabled"),
        PowerResponse::Err(e) => info!("Failed to disable boost converter: {:?}", e),
    }

    match power.enter_shipping_mode().await {
        PowerResponse::Ok => info!("Charger set to shipping mode"),
        PowerResponse::Err(e) => info!("Failed to enter shipping mode: {:?}", e),
    }

    info!("Entering deep sleep (shutdown)");
    let mut rtc = Rtc::new(peripherals.LPWR);
    rtc.sleep_deep(&[]);
}

#[embassy_executor::task]
async fn log_power_state_changes_task(mut receiver: PowerStateReceiver) {
    loop {
        let stats = receiver.changed().await.clone();
        stats.dump();
    }
}
