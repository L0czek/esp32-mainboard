# Blackbox UART Data Logger Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stream all sensor data over UART1 TX to an external blackbox device for disaster-proof recording.

**Architecture:** UART1 TX driver owned by `sensor_collection_task` (blocking writes to FIFO). Global `embassy_sync::Channel` lets other tasks enqueue packets. Sensor task drains the channel every ~10 iterations. Packet format: `SYNC(0xAA) + ID(1) + payload` with fixed or self-describing lengths.

**Tech Stack:** esp-hal UART (blocking), embassy-sync Channel, no_std Rust on ESP32-C6.

**Note:** This is a `no_std` embedded project with no test framework. Verification is `cargo build` compilation. Hardware testing happens on-device after flashing.

---

### Task 1: Add blackbox baud rate config constant

**Files:**
- Modify: `src/bin/test_stand_controller/config.rs:25` (append at end)

**Step 1: Add the constant**

Add at the end of `config.rs` (after line 25):
```rust
pub const BLACKBOX_BAUD_RATE: u32 = 921_600;
```

**Step 2: Verify build**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles without errors

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/config.rs
git commit -m "Add blackbox baud rate config constant"
```

---

### Task 2: Create blackbox module — packet types and channel

**Files:**
- Create: `src/bin/test_stand_controller/blackbox.rs`

**Step 1: Create the blackbox module with all packet types, channel, and writer**

Create `src/bin/test_stand_controller/blackbox.rs`:

```rust
use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Instant;
use esp_hal::uart::{Config, UartTx};
use esp_hal::Blocking;
use mainboard::board::D3Pin;
use mainboard::tmp107::MAX_SENSORS;

use crate::config::BLACKBOX_BAUD_RATE;

const SYNC_BYTE: u8 = 0xAA;

const ID_FAST_ADC: u8 = 0x01;
const ID_SLOW_ADC: u8 = 0x02;
const ID_TEMPERATURE: u8 = 0x03;
const ID_DIGITAL: u8 = 0x04;
const ID_SERVO: u8 = 0x05;
const ID_LOG: u8 = 0x06;
const ID_HEARTBEAT: u8 = 0x07;

const CHANNEL_CAPACITY: usize = 32;

static BLACKBOX_CHANNEL: Channel<
    CriticalSectionRawMutex,
    BlackboxPacket,
    CHANNEL_CAPACITY,
> = Channel::new();

pub enum BlackboxPacket {
    Temperature {
        count: u8,
        timestamp_ms: u32,
        values: [u16; MAX_SENSORS],
    },
    Digital {
        timestamp_ms: u32,
        value: u8,
    },
    Servo {
        timestamp_ms: u32,
        ticks: u16,
    },
    Log {
        len: u8,
        data: [u8; 64],
    },
}

pub fn send_to_blackbox(packet: BlackboxPacket) {
    if BLACKBOX_CHANNEL.try_send(packet).is_err() {
        warn!("Dropping blackbox packet: channel full");
    }
}

pub struct BlackboxWriter {
    tx: UartTx<'static, Blocking>,
}

impl BlackboxWriter {
    pub fn new(
        uart: esp_hal::peripherals::UART1<'static>,
        pin: D3Pin,
    ) -> Self {
        let tx = UartTx::new(
            uart,
            Config::default().with_baudrate(BLACKBOX_BAUD_RATE),
        )
        .expect("UART1 blackbox init failed")
        .with_tx(pin);
        Self { tx }
    }

    pub fn write_fast_adc(
        &mut self,
        ts: u32,
        tensometer: u16,
        tank: u16,
        combustion: u16,
    ) {
        let mut buf = [0u8; 12];
        buf[0] = SYNC_BYTE;
        buf[1] = ID_FAST_ADC;
        buf[2..6].copy_from_slice(&ts.to_le_bytes());
        buf[6..8].copy_from_slice(&tensometer.to_le_bytes());
        buf[8..10].copy_from_slice(&tank.to_le_bytes());
        buf[10..12].copy_from_slice(&combustion.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_slow_adc(
        &mut self,
        ts: u32,
        bat_stand: u16,
        bat_comp: u16,
        boost: u16,
        starter: u16,
    ) {
        let mut buf = [0u8; 14];
        buf[0] = SYNC_BYTE;
        buf[1] = ID_SLOW_ADC;
        buf[2..6].copy_from_slice(&ts.to_le_bytes());
        buf[6..8].copy_from_slice(&bat_stand.to_le_bytes());
        buf[8..10].copy_from_slice(&bat_comp.to_le_bytes());
        buf[10..12].copy_from_slice(&boost.to_le_bytes());
        buf[12..14].copy_from_slice(&starter.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_heartbeat(&mut self) {
        let mut buf = [0u8; 6];
        buf[0] = SYNC_BYTE;
        buf[1] = ID_HEARTBEAT;
        let ts = Instant::now().as_millis() as u32;
        buf[2..6].copy_from_slice(&ts.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_packet(&mut self, packet: &BlackboxPacket) {
        match packet {
            BlackboxPacket::Temperature {
                count,
                timestamp_ms,
                values,
            } => {
                let n = *count as usize;
                let total = 7 + n * 2;
                let mut buf = [0u8; 7 + MAX_SENSORS * 2];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_TEMPERATURE;
                buf[2] = *count;
                buf[3..7].copy_from_slice(&timestamp_ms.to_le_bytes());
                for i in 0..n {
                    let off = 7 + i * 2;
                    buf[off..off + 2]
                        .copy_from_slice(&values[i].to_le_bytes());
                }
                self.write_all(&buf[..total]);
            }
            BlackboxPacket::Digital {
                timestamp_ms,
                value,
            } => {
                let mut buf = [0u8; 7];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_DIGITAL;
                buf[2..6].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[6] = *value;
                self.write_all(&buf);
            }
            BlackboxPacket::Servo {
                timestamp_ms,
                ticks,
            } => {
                let mut buf = [0u8; 8];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_SERVO;
                buf[2..6].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[6..8].copy_from_slice(&ticks.to_le_bytes());
                self.write_all(&buf);
            }
            BlackboxPacket::Log { len, data } => {
                let n = *len as usize;
                let total = 3 + n;
                let mut buf = [0u8; 3 + 64];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_LOG;
                buf[2] = *len;
                buf[3..3 + n].copy_from_slice(&data[..n]);
                self.write_all(&buf[..total]);
            }
        }
    }

    pub fn drain_channel(&mut self) {
        while let Ok(packet) = BLACKBOX_CHANNEL.try_receive() {
            self.write_packet(&packet);
        }
    }

    fn write_all(&mut self, buf: &[u8]) {
        let mut remaining = buf;
        while !remaining.is_empty() {
            match self.tx.write(remaining) {
                Ok(n) => remaining = &remaining[n..],
                Err(_) => break,
            }
        }
    }
}
```

**Step 2: Verify build** (will fail — module not declared in main.rs yet, that's expected)

No build check yet. Proceed to Task 3.

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/blackbox.rs
git commit -m "Add blackbox module with packet format, channel, and UART writer"
```

---

### Task 3: Wire blackbox into main.rs and sensor_collection IO

**Files:**
- Modify: `src/bin/test_stand_controller/main.rs:10` (add mod declaration)
- Modify: `src/bin/test_stand_controller/main.rs:81-90` (add UART1 + D3 to sensor IO)
- Modify: `src/bin/test_stand_controller/sensor_collection.rs:16-25` (add fields to SensorCollectionIo)

**Step 1: Add `mod blackbox;` to main.rs**

In `main.rs`, after line 10 (`mod armed;`), add:
```rust
mod blackbox;
```

**Step 2: Add UART1 and D3 fields to SensorCollectionIo**

In `sensor_collection.rs`, add two new fields to `SensorCollectionIo` (after line 24 `pub boost_voltage: BoostVolPin,`):
```rust
    pub uart1: esp_hal::peripherals::UART1<'static>,
    pub blackbox_tx_pin: mainboard::board::D3Pin,
```

**Step 3: Pass UART1 and D3 in main.rs**

In `main.rs`, modify the `sensor_collection_io` construction (lines 81-90). Add after `boost_voltage: board.BoostVol,` (line 89):
```rust
        uart1: peripherals.UART1,
        blackbox_tx_pin: board.D3,
```

**Step 4: Verify build**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles (blackbox module is declared, SensorCollectionIo has new fields, but they're not used yet — that's fine, just unused warnings)

**Step 5: Commit**

```bash
git add src/bin/test_stand_controller/main.rs src/bin/test_stand_controller/sensor_collection.rs
git commit -m "Wire blackbox module and UART1 IO into sensor collection"
```

---

### Task 4: Integrate blackbox writer into sensor_collection_task

This is the core integration. The sensor task owns the `BlackboxWriter` and writes fast/slow ADC packets directly. It drains the blackbox channel every 10 fast iterations and after slow reads.

**Files:**
- Modify: `src/bin/test_stand_controller/sensor_collection.rs`

**Step 1: Initialize BlackboxWriter in task body**

In `sensor_collection_task` (line 87-93), after `let mut state = SensorCollectionState::new(io);` but note that `io` is consumed by `SensorCollectionState::new`. We need to extract `uart1` and `blackbox_tx_pin` from `io` BEFORE passing it to `SensorCollectionState::new`.

Change the `SensorCollectionState::new` call. Currently (line 88):
```rust
let mut state = SensorCollectionState::new(io);
```

Replace with:
```rust
let uart1 = io.uart1;
let blackbox_tx_pin = io.blackbox_tx_pin;
let mut state = SensorCollectionState::new(io);
let mut blackbox = crate::blackbox::BlackboxWriter::new(uart1, blackbox_tx_pin);
```

Wait — this won't work because `io` is moved into `SensorCollectionState::new()`. We need to destructure or extract UART fields first. The cleanest approach: extract the two blackbox fields before passing the rest.

Actually, change `SensorCollectionState::new` to not consume the UART fields. The simplest fix: extract the fields before the call:

```rust
pub async fn sensor_collection_task(io: SensorCollectionIo) {
    let mut blackbox = crate::blackbox::BlackboxWriter::new(
        io.uart1,
        io.blackbox_tx_pin,
    );
    let mut state = SensorCollectionState::new(io);
```

But `io` is partially moved. Rust won't allow using `io` after moving fields out. We need to restructure. The cleanest way: separate the blackbox IO from sensor IO, or destructure `io` manually.

Best approach — split `SensorCollectionIo` fields out manually:

Replace the entire task function (lines 87-93):

```rust
#[embassy_executor::task]
pub async fn sensor_collection_task(io: SensorCollectionIo) {
    let mut blackbox = crate::blackbox::BlackboxWriter::new(
        io.uart1,
        io.blackbox_tx_pin,
    );
    let mut state = SensorCollectionState::new(
        io.adc,
        io.tensometer,
        io.pressure_tank,
        io.pressure_combustion,
        io.starter_sense,
        io.battery_stand,
        io.battery_computer,
        io.boost_voltage,
    );

    loop {
        collect_and_publish_fast(&mut state, &mut blackbox).await;
        collect_and_publish_slow(&mut state, &mut blackbox);
    }
}
```

And change `SensorCollectionState::new` to accept individual fields instead of consuming the whole IO struct. Replace the current `SensorCollectionState::new` (lines 39-84):

```rust
impl SensorCollectionState {
    fn new(
        adc: ADC1<'static>,
        tensometer: A0Pin,
        pressure_tank: A1Pin,
        pressure_combustion: A2Pin,
        starter_sense: A3Pin,
        battery_stand: A4Pin,
        battery_computer: BatVolPin,
        boost_voltage: BoostVolPin,
    ) -> Self {
        let mut config = AdcConfig::new();

        let tensometer =
            config.enable_pin_with_cal::<A0Pin, AdcCalBasic<ADC1<'static>>>(
                tensometer,
                Attenuation::_0dB,
            );
        let pressure_tank =
            config.enable_pin_with_cal::<A1Pin, AdcCalBasic<ADC1<'static>>>(
                pressure_tank,
                Attenuation::_0dB,
            );
        let pressure_combustion =
            config.enable_pin_with_cal::<A2Pin, AdcCalBasic<ADC1<'static>>>(
                pressure_combustion,
                Attenuation::_0dB,
            );
        let starter_sense =
            config.enable_pin_with_cal::<A3Pin, AdcCalBasic<ADC1<'static>>>(
                starter_sense,
                Attenuation::_0dB,
            );
        let battery_stand =
            config.enable_pin_with_cal::<A4Pin, AdcCalBasic<ADC1<'static>>>(
                battery_stand,
                Attenuation::_0dB,
            );
        let battery_computer =
            config.enable_pin_with_cal::<BatVolPin, AdcCalBasic<ADC1<'static>>>(
                battery_computer,
                Attenuation::_0dB,
            );
        let boost_voltage =
            config.enable_pin_with_cal::<BoostVolPin, AdcCalBasic<ADC1<'static>>>(
                boost_voltage,
                Attenuation::_0dB,
            );

        let adc = Adc::new(adc, config);

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
```

**Step 2: Modify `collect_and_publish_fast` to write blackbox packets**

Change function signature from:
```rust
async fn collect_and_publish_fast(state: &mut SensorCollectionState) {
```
To:
```rust
async fn collect_and_publish_fast(
    state: &mut SensorCollectionState,
    blackbox: &mut crate::blackbox::BlackboxWriter,
) {
```

Inside the `for index in 0..FAST_BATCH_SAMPLES` loop, after reading the three ADC channels (after line 113), add:

```rust
        blackbox.write_fast_adc(
            timestamp_ms,
            tensometer[index],
            pressure_tank[index],
            pressure_combustion[index],
        );

        if index % 10 == 0 {
            blackbox.drain_channel();
        }
```

**Step 3: Modify `collect_and_publish_slow` to write blackbox packets**

Change function signature from:
```rust
fn collect_and_publish_slow(state: &mut SensorCollectionState) {
```
To:
```rust
fn collect_and_publish_slow(
    state: &mut SensorCollectionState,
    blackbox: &mut crate::blackbox::BlackboxWriter,
) {
```

After reading all 4 slow channels (after line 178, the `starter_sense` read), before constructing `SlowSensorsBatch`, add:

```rust
    blackbox.write_slow_adc(
        battery_stand.timestamp_ms,
        battery_stand.value,
        battery_computer.value,
        boost_voltage.value,
        starter_sense.value,
    );
    blackbox.drain_channel();
```

Note: `SlowAdcPacket` has `pub timestamp_ms: u32` and `pub value: u16` fields, so we can access them directly.

**Step 4: Verify build**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles without errors

**Step 5: Commit**

```bash
git add src/bin/test_stand_controller/sensor_collection.rs
git commit -m "Integrate blackbox writer into sensor collection task"
```

---

### Task 5: Integrate blackbox into temperature_collection_task

**Files:**
- Modify: `src/bin/test_stand_controller/temperature_collection.rs:70-76`

**Step 1: Add blackbox send after temperature read**

After the successful `read_all_temperatures` call (line 70-76), after `let count = match ...` and after `show_address_leds` (line 80), add:

```rust
        let mut bb_values = [0u16; MAX_SENSORS];
        bb_values[..count].copy_from_slice(&read_buf[..count]);
        crate::blackbox::send_to_blackbox(
            crate::blackbox::BlackboxPacket::Temperature {
                count: count as u8,
                timestamp_ms: Instant::now().as_millis() as u32,
                values: bb_values,
            },
        );
```

Place this right after line 80 (the `show_address_leds` error handling) and before line 82 (`let now = ...`). We can reuse the `now` timestamp by moving this after line 82 and using `now` instead:

Actually, place it after line 82 (`let now = ...`) to reuse the timestamp:

```rust
        let mut bb_values = [0u16; MAX_SENSORS];
        bb_values[..count].copy_from_slice(&read_buf[..count]);
        crate::blackbox::send_to_blackbox(
            crate::blackbox::BlackboxPacket::Temperature {
                count: count as u8,
                timestamp_ms: now,
                values: bb_values,
            },
        );
```

Insert after line 82 (`let now = Instant::now().as_millis() as u32;`).

**Step 2: Verify build**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles without errors

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/temperature_collection.rs
git commit -m "Send temperature readings to blackbox"
```

---

### Task 6: Integrate blackbox into armed_monitor_task

**Files:**
- Modify: `src/bin/test_stand_controller/armed.rs:35-44`

**Step 1: Add blackbox send in `publish_armed_state`**

In `publish_armed_state` (line 35-44), after the MQTT publish (after line 43), add:

```rust
    crate::blackbox::send_to_blackbox(
        crate::blackbox::BlackboxPacket::Digital {
            timestamp_ms: timestamp_ms(),
            value,
        },
    );
```

Insert after line 43 (closing brace of the `if` block), before line 44 (closing brace of the function).

**Step 2: Verify build**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles without errors

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/armed.rs
git commit -m "Send armed state changes to blackbox"
```

---

### Task 7: Integrate blackbox into servo_controller_task

**Files:**
- Modify: `src/bin/test_stand_controller/servo.rs:74-81`

**Step 1: Add blackbox send in `publish_servo_position`**

In `publish_servo_position` (line 74-81), after the MQTT publish (after line 80), add:

```rust
    crate::blackbox::send_to_blackbox(
        crate::blackbox::BlackboxPacket::Servo {
            timestamp_ms,
            ticks,
        },
    );
```

Insert after line 80 (closing brace of the `if` block), before line 81 (closing brace of the function).

**Step 2: Verify build**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles without errors

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/servo.rs
git commit -m "Send servo position updates to blackbox"
```

---

### Task 8: Final build verification and format check

**Files:** None (verification only)

**Step 1: Full build**

Run: `cargo build 2>&1`
Expected: compiles without errors

**Step 2: Check for warnings**

Run: `cargo build 2>&1 | grep -i warning`
Expected: no warnings (fix any that appear)

**Step 3: Format check**

Run: `cargo fmt -- --check`
Expected: no formatting issues (run `cargo fmt` if needed)

**Step 4: Clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: no errors (fix any clippy warnings)

---

### Task 9: Update documentation

**Files:**
- Modify: `AGENTS.md` (add blackbox module description)

**Step 1: Add blackbox section to AGENTS.md**

Add a section describing the new blackbox module:
- Module location: `src/bin/test_stand_controller/blackbox.rs`
- Purpose: UART1 TX data logger for external blackbox device
- Packet format summary with ID table
- Channel architecture (which tasks send, sensor_collection drains)
- Pin: D3/GPIO20, configurable baud rate in config.rs

**Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "Document blackbox UART data logger in AGENTS.md"
```
