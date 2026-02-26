# Servo Controller Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Drive a pulse-width servo on GPIO22 (D1) via MCPWM, controlled by MQTT open/close commands with slow linear interpolation and position telemetry.

**Architecture:** Dedicated Embassy task owns MCPWM0 peripheral, receives ServoCommand via embassy_sync Channel from the MQTT handler, interpolates linearly between positions at ~20ms tick intervals, publishes ServoStatus transitions and ServoSensorPacket position data through the existing outbound MQTT queue.

**Tech Stack:** esp-hal MCPWM (ESP32-C6), embassy-executor, embassy-sync Channel, embassy-time Timer, embassy-futures select

**Design doc:** `docs/plans/2026-02-26-servo-controller-design.md`

---

### Task 1: Add servo configuration constants

**Files:**
- Modify: `src/bin/test_stand_controller/config.rs`

**Step 1: Add servo constants**

Append to end of `config.rs`:

```rust
// Servo pulse width range (MCPWM ticks mapping physical 0-180 degrees)
pub const SERVO_MIN_PULSE_TICKS: u16 = 500;
pub const SERVO_MAX_PULSE_TICKS: u16 = 2500;

// Operational positions (degrees within the 0-180 range)
pub const SERVO_OPEN_DEGREES: u16 = 90;
pub const SERVO_CLOSED_DEGREES: u16 = 0;

// Time for full 0-180 degree travel
pub const SERVO_FULL_RANGE_MS: u64 = 5000;
```

**Step 2: Verify it compiles**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo check --bin test_stand_controller`
Expected: compiles (config.rs has no new imports)

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/config.rs
git commit -m "Add servo configuration constants"
```

---

### Task 2: Create servo controller module with MCPWM init and command channel

**Files:**
- Create: `src/bin/test_stand_controller/servo.rs`

**Step 1: Create servo.rs with channel, helpers, and task skeleton**

Create `src/bin/test_stand_controller/servo.rs` with this content:

```rust
use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::mcpwm::operator::PwmPinConfig;
use esp_hal::mcpwm::timer::PwmWorkingMode;
use esp_hal::mcpwm::{McPwm, PeripheralClockConfig};
use esp_hal::peripherals::MCPWM0;
use esp_hal::time::Rate;
use mainboard::board::D1Pin;

use crate::config::{
    SERVO_CLOSED_DEGREES, SERVO_FULL_RANGE_MS, SERVO_MAX_PULSE_TICKS,
    SERVO_MIN_PULSE_TICKS, SERVO_OPEN_DEGREES,
};
use crate::mqtt::commands::servo::ServoCommand;
use crate::mqtt::queue;
use crate::mqtt::sensors::slow::ServoSensorPacket;
use crate::mqtt::sensors::status::ServoStatus;

const TICK_INTERVAL_MS: u64 = 20;

static SERVO_COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, ServoCommand, 4> =
    Channel::new();

pub fn send_servo_command(command: ServoCommand) {
    if SERVO_COMMAND_CHANNEL.try_send(command).is_err() {
        warn!("Servo command channel full, dropping command");
    }
}

fn degrees_to_ticks(degrees: u16) -> u16 {
    let range = SERVO_MAX_PULSE_TICKS - SERVO_MIN_PULSE_TICKS;
    SERVO_MIN_PULSE_TICKS + ((degrees as u32 * range as u32) / 180) as u16
}

fn travel_time_ms(from_ticks: u16, to_ticks: u16) -> u64 {
    let tick_range = (SERVO_MAX_PULSE_TICKS - SERVO_MIN_PULSE_TICKS) as u64;
    let distance = from_ticks.abs_diff(to_ticks) as u64;
    SERVO_FULL_RANGE_MS * distance / tick_range
}

fn target_ticks_for_command(command: ServoCommand) -> u16 {
    match command {
        ServoCommand::Open => degrees_to_ticks(SERVO_OPEN_DEGREES),
        ServoCommand::Close => degrees_to_ticks(SERVO_CLOSED_DEGREES),
    }
}

fn status_for_command(command: ServoCommand) -> (ServoStatus, ServoStatus) {
    match command {
        ServoCommand::Open => (ServoStatus::Opening, ServoStatus::Open),
        ServoCommand::Close => (ServoStatus::Closing, ServoStatus::Closed),
    }
}

fn publish_servo_status(status: ServoStatus) {
    if queue::publish_servo_status(status).is_err() {
        warn!("Failed to publish servo status: queue full");
    }
}

fn publish_servo_position(ticks: u16) {
    let timestamp_ms = Instant::now().as_millis() as u32;
    let packet = ServoSensorPacket::new(timestamp_ms, ticks);
    if queue::publish_servo_sensor(packet).is_err() {
        warn!("Failed to publish servo position: queue full");
    }
}

#[embassy_executor::task]
pub async fn servo_controller_task(mcpwm: MCPWM0, pin: D1Pin) {
    let clock_cfg = PeripheralClockConfig::with_frequency(
        Rate::from_mhz(32),
    )
    .expect("Failed to configure MCPWM clock");

    let mut mcpwm = McPwm::new(mcpwm, clock_cfg);
    mcpwm.operator0.set_timer(&mcpwm.timer0);

    let mut pwm_pin = mcpwm
        .operator0
        .with_pin_a(pin, PwmPinConfig::UP_ACTIVE_HIGH);

    let timer_clock_cfg = clock_cfg
        .timer_clock_with_frequency(
            19_999,
            PwmWorkingMode::Increase,
            Rate::from_hz(50),
        )
        .expect("Failed to configure MCPWM timer");
    mcpwm.timer0.start(timer_clock_cfg);

    // Boot: drive to closed position
    let closed_ticks = degrees_to_ticks(SERVO_CLOSED_DEGREES);
    let mut current_ticks = closed_ticks;
    pwm_pin.set_timestamp(current_ticks);
    publish_servo_status(ServoStatus::Closed);
    publish_servo_position(current_ticks);
    info!("Servo initialized at closed position ({} ticks)", current_ticks);

    loop {
        // Idle: wait for a command
        let command = SERVO_COMMAND_CHANNEL.receive().await;
        let target_ticks = target_ticks_for_command(command);

        if target_ticks == current_ticks {
            continue;
        }

        let (moving_status, arrived_status) = status_for_command(command);
        publish_servo_status(moving_status);

        let total_time_ms = travel_time_ms(current_ticks, target_ticks);
        let total_steps = total_time_ms / TICK_INTERVAL_MS;
        let start_ticks = current_ticks;

        let mut step: u64 = 0;
        let mut reached = false;

        while !reached {
            match select(
                SERVO_COMMAND_CHANNEL.receive(),
                Timer::after(Duration::from_millis(TICK_INTERVAL_MS)),
            )
            .await
            {
                Either::First(new_command) => {
                    let new_target = target_ticks_for_command(new_command);
                    if new_target == current_ticks {
                        let (_, new_arrived) = status_for_command(new_command);
                        publish_servo_status(new_arrived);
                        reached = true;
                        continue;
                    }
                    if new_target == target_ticks {
                        continue;
                    }
                    // Restart interpolation from current position
                    // Re-enter the outer loop by breaking, then
                    // the outer loop will process via channel re-send
                    send_servo_command(new_command);
                    let (_, mid_status) = status_for_command(new_command);
                    // Publish intermediate status — the outer loop
                    // will pick up the re-sent command and publish
                    // the moving status
                    reached = true;
                    let _ = mid_status;
                    continue;
                }
                Either::Second(()) => {
                    step += 1;
                    if step >= total_steps {
                        current_ticks = target_ticks;
                        reached = true;
                    } else {
                        let progress = step as i32;
                        let total = total_steps as i32;
                        let delta =
                            (target_ticks as i32 - start_ticks as i32) * progress / total;
                        current_ticks = (start_ticks as i32 + delta) as u16;
                    }
                    pwm_pin.set_timestamp(current_ticks);
                    publish_servo_position(current_ticks);
                }
            }
        }

        if current_ticks == target_ticks {
            publish_servo_status(arrived_status);
        }
    }
}
```

**Step 2: Verify it compiles**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo check --bin test_stand_controller`
Expected: May fail — `servo.rs` isn't wired into `main.rs` yet, and `queue::publish_servo_sensor` may not exist. Proceed to Task 3.

---

### Task 3: Add publish_servo_sensor to MQTT queue

**Files:**
- Modify: `src/bin/test_stand_controller/mqtt/queue.rs`

**Step 1: Add publish_servo_sensor function**

The queue already has `publish_servo_status` (line 128) for the `ServoStatus` enum. Add a function for individual `ServoSensorPacket` publishing. Append after the `publish_servo_status` function (after line 130):

```rust
pub fn publish_servo_sensor(packet: ServoSensorPacket) -> Result<(), PublishError> {
    enqueue(OutboundMessage::ServoSensor(packet))
}
```

Note: `ServoSensorPacket` is already imported via `use crate::mqtt::sensors::slow::ServoSensorPacket;` (line 5), and `OutboundMessage::ServoSensor` already exists (line 20). This function is the missing convenience wrapper.

**Step 2: Commit**

```bash
git add src/bin/test_stand_controller/mqtt/queue.rs
git commit -m "Add publish_servo_sensor convenience function to MQTT queue"
```

---

### Task 4: Wire servo module into main.rs

**Files:**
- Modify: `src/bin/test_stand_controller/main.rs`

**Step 1: Add mod declaration**

Add `mod servo;` after `mod sensor_collection;` (line 12):

```rust
mod sensor_collection;
mod servo;
mod wifi;
```

**Step 2: Spawn servo task**

After the sensor collection spawn block (after line 125 `info!("Sensor collection task spawned");`), add:

```rust
    spawner
        .spawn(servo::servo_controller_task(
            peripherals.MCPWM0,
            board.D1,
        ))
        .expect("Failed to spawn servo_controller_task");
    info!("Servo controller task spawned");
```

**Step 3: Verify it compiles**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo check --bin test_stand_controller`
Expected: PASS (all pieces connected)

**Step 4: Commit**

```bash
git add src/bin/test_stand_controller/main.rs
git commit -m "Wire servo controller task into main"
```

---

### Task 5: Wire MQTT servo command handler to channel

**Files:**
- Modify: `src/bin/test_stand_controller/mqtt/client.rs`

**Step 1: Update ServoCommandHandler impl**

Replace the `impl ServoCommandHandler for AppCommandHandlers` block (lines 88-100) with:

```rust
impl ServoCommandHandler for AppCommandHandlers {
    fn handle_servo_command(&mut self, command: ServoCommand) {
        if self.state == StateStatus::Fire {
            warn!("MQTT command ignored: cmd/servo in FIRE state");
            return;
        }

        match command {
            ServoCommand::Open => info!("MQTT command: OPEN"),
            ServoCommand::Close => info!("MQTT command: CLOSE"),
        }
        crate::servo::send_servo_command(command);
    }
}
```

The only change is adding the `crate::servo::send_servo_command(command);` call after the existing logging.

**Step 2: Verify it compiles**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo check --bin test_stand_controller`
Expected: PASS

**Step 3: Commit**

```bash
git add src/bin/test_stand_controller/mqtt/client.rs
git commit -m "Route MQTT servo commands to servo controller channel"
```

---

### Task 6: Lint and final verification

**Step 1: Run cargo fmt**

Run: `cargo fmt --all`

**Step 2: Run clippy**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo clippy --bin test_stand_controller -- -D warnings`
Expected: No warnings

**Step 3: Fix any issues found by clippy**

Address all warnings/errors.

**Step 4: Final compile check**

Run: `WIFI_SSID=x WIFI_PASSWORD=x MQTT_HOST=x cargo build --release --bin test_stand_controller`
Expected: PASS

**Step 5: Commit any formatting/lint fixes**

```bash
git add -A
git commit -m "Fix formatting and lint warnings in servo controller"
```

---

### Task 7: Update AGENTS.md and README.md

**Files:**
- Modify: `AGENTS.md`

**Step 1: Add servo controller documentation to AGENTS.md**

Add a section describing:
- The servo module (`src/bin/test_stand_controller/servo.rs`)
- MCPWM configuration (MCPWM0, GPIO22/D1, 50Hz, up-counting active high)
- Command channel pattern (Channel<ServoCommand, 4>)
- Linear interpolation at 20ms tick interval
- Config constants in config.rs (pulse ticks, degrees, travel time)
- Publishing: ServoStatus on transitions, ServoSensorPacket on each tick

**Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "Document servo controller in AGENTS.md"
```
