# TMP107 One-Shot + Shutdown with Batched Reads — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Switch TMP107 from continuous conversion to one-shot + shutdown mode with 20Hz sampling and batched MQTT publishing (20 readings per packet, once per second).

**Architecture:** Driver gets two new public methods (`shutdown`, `trigger_one_shot`) that write the configuration register via the existing `global_write` primitive. Collection task loop triggers one-shot, waits 20ms for conversion, reads all temps, accumulates 20 samples, then publishes one batched TempPacket per sensor.

**Tech Stack:** Rust, esp-hal 1.0, Embassy async, `no_std`

---

### Task 1: Add one-shot + shutdown methods to TMP107 driver

**Files:**
- Modify: `src/tmp107.rs:9-22` (constants section)
- Modify: `src/tmp107.rs:39-84` (public API section)
- Modify: `src/tmp107.rs:259` (remove `#[allow(dead_code)]` from `global_write`)

**Step 1: Add constants after line 16**

Add these constants after `TEMP_REGISTER`:

```rust
/// Configuration register address.
const CONFIG_REGISTER: u8 = 0x01;

/// Config: SD=1 (shutdown mode), all other bits 0.
const CONFIG_SHUTDOWN: u16 = 0x0800;

/// Config: SD=1 + OS=1 (trigger one-shot conversion from shutdown).
const CONFIG_ONESHOT: u16 = 0x1800;
```

**Step 2: Add public methods after `read_all_temperatures` (after line 84)**

```rust
/// Put all sensors into shutdown mode (stops continuous conversion).
/// Call once after init before starting one-shot collection loop.
pub async fn shutdown(&mut self) -> Result<(), Tmp107Error> {
    self.global_write(self.sensor_count, CONFIG_REGISTER, CONFIG_SHUTDOWN)
        .await
}

/// Trigger a single temperature conversion on all sensors.
/// Sensors return to shutdown mode after conversion completes.
/// Wait at least 20ms before reading results.
pub async fn trigger_one_shot(&mut self) -> Result<(), Tmp107Error> {
    self.global_write(self.sensor_count, CONFIG_REGISTER, CONFIG_ONESHOT)
        .await
}
```

**Step 3: Remove `#[allow(dead_code)]` from `global_write` (line 259)**

Delete the `#[allow(dead_code)]` line above `async fn global_write`. It is now used by the public methods.

**Step 4: Compile check**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo clippy --bin test_stand_controller -- -D warnings`
Expected: Clean compile, zero warnings.

**Step 5: Commit**

```bash
git add src/tmp107.rs
git commit -m "Add shutdown and trigger_one_shot methods to TMP107 driver"
```

---

### Task 2: Update config constants

**Files:**
- Modify: `src/bin/test_stand_controller/config.rs:11` (change interval, add new constants)

**Step 1: Replace line 11 and add new constants**

Replace:
```rust
pub const TEMP_COLLECTION_INTERVAL_MS: u64 = 100;
```

With:
```rust
pub const TEMP_COLLECTION_INTERVAL_MS: u64 = 50;
pub const TEMP_BATCH_SIZE: usize = 20;
pub const ONESHOT_CONVERSION_MS: u64 = 20;
```

**Step 2: Compile check**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo check --bin test_stand_controller`
Expected: Clean compile (unused warnings OK for now since collection task hasn't changed yet).

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/config.rs
git commit -m "Update temp config: 20Hz interval, batch size 20, oneshot delay"
```

---

### Task 3: Rewrite temperature collection task for one-shot + batching

**Files:**
- Modify: `src/bin/test_stand_controller/temperature_collection.rs` (full rewrite of task body)

**Step 1: Rewrite the file**

Replace the entire file content with:

```rust
use defmt::{error, info, warn};
use embassy_time::{Duration, Instant, Ticker, Timer};
use esp_hal::peripherals::UART0;
use esp_hal::uart::Uart;

use crate::config::{
    ONESHOT_CONVERSION_MS, TEMP_BATCH_SIZE, TEMP_COLLECTION_INTERVAL_MS,
};
use crate::mqtt::publish_temperature_sensor;
use crate::mqtt::sensors::temp::TempPacket;
use mainboard::board::{D0Pin, U0RxPin, U0TxPin};
use mainboard::tmp107::{Tmp107, MAX_SENSORS};

pub struct TemperatureCollectionIo {
    pub uart: UART0<'static>,
    pub tx_pin: U0TxPin,
    pub rx_pin: U0RxPin,
    pub dir_pin: D0Pin,
}

#[embassy_executor::task]
pub async fn temperature_collection_task(io: TemperatureCollectionIo) {
    let uart = Uart::new(
        io.uart,
        esp_hal::uart::Config::default().with_baudrate(115200),
    )
    .expect("UART0 init failed")
    .with_tx(io.tx_pin)
    .with_rx(io.rx_pin)
    .with_dtr(io.dir_pin)
    .with_rs485()
    .into_async();

    let (rx, tx) = uart.split();

    let mut driver = match Tmp107::init(tx, rx).await {
        Ok(d) => d,
        Err(e) => {
            error!("TMP107 init failed: {:?}", e);
            return;
        }
    };

    let sensor_count = driver.sensor_count() as usize;

    if let Err(e) = driver.shutdown().await {
        error!("TMP107 shutdown failed: {:?}", e);
        return;
    }

    info!(
        "Temperature collection: {} sensors, {}ms interval, batch {}",
        sensor_count, TEMP_COLLECTION_INTERVAL_MS, TEMP_BATCH_SIZE,
    );

    let mut ticker = Ticker::every(Duration::from_millis(
        TEMP_COLLECTION_INTERVAL_MS,
    ));
    let mut read_buf = [0u16; MAX_SENSORS];
    let mut batch = [[0u16; TEMP_BATCH_SIZE]; MAX_SENSORS];
    let mut sample_index: usize = 0;
    let mut first_timestamp_ms: u32 = 0;

    loop {
        ticker.next().await;

        if let Err(e) = driver.trigger_one_shot().await {
            warn!("TMP107 one-shot trigger failed: {:?}", e);
            continue;
        }

        Timer::after_millis(ONESHOT_CONVERSION_MS).await;

        let count = match driver.read_all_temperatures(&mut read_buf).await
        {
            Ok(n) => n,
            Err(e) => {
                warn!("TMP107 read failed: {:?}", e);
                continue;
            }
        };

        let now = Instant::now().as_millis() as u32;

        if sample_index == 0 {
            first_timestamp_ms = now;
        }

        for sensor in 0..count {
            batch[sensor][sample_index] = read_buf[sensor];
        }
        sample_index += 1;

        if sample_index >= TEMP_BATCH_SIZE {
            for sensor in 0..count {
                let sensor_id = (sensor + 1) as u8;
                let packet = match TempPacket::from_slice(
                    sensor_id,
                    first_timestamp_ms,
                    now,
                    &batch[sensor][..TEMP_BATCH_SIZE],
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(
                            "TMP107 packet error sensor {}: {:?}",
                            sensor_id, e,
                        );
                        continue;
                    }
                };

                if publish_temperature_sensor(packet).is_err() {
                    warn!("Dropping temp packet: queue full");
                }
            }
            sample_index = 0;
        }
    }
}
```

Key changes from old version:
- Calls `driver.shutdown()` once after init
- Each tick: `trigger_one_shot()` → wait 20ms → `read_all_temperatures()`
- Accumulates readings into `batch[sensor][sample_index]`
- After 20 samples: publishes one `TempPacket` per sensor with all 20 values and first/last timestamps
- Resets `sample_index` to 0 after publishing

**Step 2: Compile and clippy check**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo clippy --bin test_stand_controller -- -D warnings`
Expected: Clean compile, zero warnings.

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/temperature_collection.rs
git commit -m "Switch temp collection to one-shot + shutdown with 20-sample batching"
```

---

### Task 4: Update AGENTS.md

**Files:**
- Modify: `AGENTS.md` — update TMP107 section to reflect one-shot mode and batching

Update the TMP107 section to mention:
- One-shot + shutdown mode (not continuous conversion)
- 20Hz sampling rate
- 20-sample batched TempPackets published once per second
- Config constants: `TEMP_COLLECTION_INTERVAL_MS`, `TEMP_BATCH_SIZE`, `ONESHOT_CONVERSION_MS`

**Step 1: Edit the relevant section in AGENTS.md**

**Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "Update AGENTS.md with one-shot mode and batching details"
```
