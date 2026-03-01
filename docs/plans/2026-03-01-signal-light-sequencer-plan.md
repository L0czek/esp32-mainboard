# Signal Light Driver & State Sequencer Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a PCF8574-based signalling light driver and replace the armed monitor with a state sequencer task that controls rocket state transitions and signal light behavior.

**Architecture:** New `signal_light` module at lib level wraps PCF8574 with typed config. New `sequencer` module in test_stand_controller replaces `armed.rs`, owns the safety switch pin + signal light, and receives state commands via a channel from MQTT. MQTT client delegates state management to the sequencer instead of handling it inline.

**Tech Stack:** `pcf857x` crate (already in deps), `embassy_sync::channel`, `embassy_futures::select`, `embassy_time::Timer`

---

### Task 1: Create SignalLightConfig type

**Files:**
- Create: `src/signal_light.rs`
- Modify: `src/lib.rs:4-8` (add module declaration)

**Step 1: Create `src/signal_light.rs` with `SignalLightConfig`**

```rust
use defmt::Format;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Format)]
pub struct SignalLightConfig {
    pub green: bool,
    pub red: bool,
    pub yellow: bool,
    pub blue: bool,
    pub white: bool,
    pub buzzer: bool,
}

impl Default for SignalLightConfig {
    fn default() -> Self {
        Self {
            green: false,
            red: false,
            yellow: false,
            blue: false,
            white: false,
            buzzer: false,
        }
    }
}

impl SignalLightConfig {
    /// Convert to PCF8574 register byte.
    /// All outputs are active-low: 0 = on, 1 = off.
    /// Bits 6-7 (reserved) are kept high (off).
    pub fn to_register(self) -> u8 {
        let mut reg: u8 = 0xFF;
        if self.green {
            reg &= !(1 << 0);
        }
        if self.red {
            reg &= !(1 << 1);
        }
        if self.yellow {
            reg &= !(1 << 2);
        }
        if self.blue {
            reg &= !(1 << 3);
        }
        if self.white {
            reg &= !(1 << 4);
        }
        if self.buzzer {
            reg &= !(1 << 5);
        }
        reg
    }

    /// Parse from PCF8574 register byte (active-low).
    pub fn from_register(reg: u8) -> Self {
        Self {
            green: reg & (1 << 0) == 0,
            red: reg & (1 << 1) == 0,
            yellow: reg & (1 << 2) == 0,
            blue: reg & (1 << 3) == 0,
            white: reg & (1 << 4) == 0,
            buzzer: reg & (1 << 5) == 0,
        }
    }
}
```

**Step 2: Add module to `src/lib.rs`**

Add `pub mod signal_light;` after the existing module declarations.

**Step 3: Verify it compiles**

Run: `cargo check --bin test_stand_controller`
Expected: PASS (no errors)

**Step 4: Commit**

```
feat: add SignalLightConfig type with active-low register conversion
```

---

### Task 2: Create SignalLight driver struct

**Files:**
- Modify: `src/signal_light.rs` (append driver struct)

**Step 1: Add the `SignalLight` struct below `SignalLightConfig`**

```rust
use embedded_hal::i2c::I2c;
use pcf857x::Pcf8574;

pub struct SignalLight<I2C: I2c> {
    expander: Pcf8574<I2C>,
    current: SignalLightConfig,
}

impl<I2C: I2c> SignalLight<I2C> {
    pub fn new(
        i2c: I2C,
        address: pcf857x::SlaveAddr,
    ) -> Result<Self, pcf857x::Error<I2C::Error>> {
        let mut expander = Pcf8574::new(i2c, address);
        let config = SignalLightConfig::default();
        expander.set(config.to_register())?;
        Ok(Self {
            expander,
            current: config,
        })
    }

    pub fn set(
        &mut self,
        config: SignalLightConfig,
    ) -> Result<(), pcf857x::Error<I2C::Error>> {
        self.expander.set(config.to_register())?;
        self.current = config;
        Ok(())
    }

    pub fn current(&self) -> SignalLightConfig {
        self.current
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check --bin test_stand_controller`
Expected: PASS

**Step 3: Commit**

```
feat: add SignalLight driver wrapping PCF8574
```

---

### Task 3: Create sequencer module skeleton with channel

**Files:**
- Create: `src/bin/test_stand_controller/sequencer.rs`
- Modify: `src/bin/test_stand_controller/main.rs:10` (replace `mod armed` with `mod sequencer`)

**Step 1: Create `sequencer.rs` with channel and public API**

```rust
use core::sync::atomic::{AtomicU8, Ordering};

use defmt::{info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use embassy_futures::select::{select, Either};
use esp_hal::gpio::Input;
use mainboard::board::I2cType;
use mainboard::signal_light::{SignalLight, SignalLightConfig};

use crate::mqtt::commands::state::StateCommand;
use crate::mqtt::sensors::digital::ArmedPacket;
use crate::mqtt::sensors::status::StateStatus;
use crate::mqtt::queue;

const FIRE_BUZZER_DURATION_MS: u64 = 3000;

static SEQUENCER_COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, StateCommand, 4> =
    Channel::new();
static LAST_ARMED_VALUE: AtomicU8 = AtomicU8::new(0);
static CURRENT_STATE: AtomicU8 = AtomicU8::new(0);

pub fn send_state_command(command: StateCommand) {
    if SEQUENCER_COMMAND_CHANNEL.try_send(command).is_err() {
        warn!("Sequencer command channel full, dropping command");
    }
}

pub fn init_armed_state(pin: &Input<'_>) {
    let value = pin.is_high() as u8;
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    info!("Armed switch initial state: {}", value);
}

fn store_state(status: StateStatus) {
    let v = match status {
        StateStatus::Armed => 0,
        StateStatus::Fire => 1,
        StateStatus::PostFire => 2,
    };
    CURRENT_STATE.store(v, Ordering::Relaxed);
}

pub fn load_state() -> StateStatus {
    match CURRENT_STATE.load(Ordering::Relaxed) {
        1 => StateStatus::Fire,
        2 => StateStatus::PostFire,
        _ => StateStatus::Armed,
    }
}

pub fn republish_sequencer_state() {
    let _ = queue::publish_state_status(load_state());
}

pub fn republish_armed_state() {
    let value = LAST_ARMED_VALUE.load(Ordering::Relaxed);
    let packet = ArmedPacket::new(timestamp_ms(), value);
    let _ = crate::mqtt::publish_armed_sensor(packet);
}

fn timestamp_ms() -> u32 {
    Instant::now().as_millis() as u32
}
```

**Step 2: Replace `mod armed` with `mod sequencer` in `main.rs` line 10**

Change:
```rust
mod armed;
```
To:
```rust
mod sequencer;
```

**Step 3: Verify it compiles (will have errors from references to armed:: — expected)**

Run: `cargo check --bin test_stand_controller`
Expected: errors referencing `armed::` in main.rs and mqtt/client.rs — that's fine, we fix those next.

**Step 4: Commit (even with errors — this is a checkpoint)**

Actually, don't commit yet. Continue to Task 4 first so we get to a compiling state.

---

### Task 4: Wire sequencer into main.rs and mqtt client

**Files:**
- Modify: `src/bin/test_stand_controller/main.rs` (replace armed task spawn with sequencer)
- Modify: `src/bin/test_stand_controller/mqtt/client.rs` (delegate state commands to sequencer)

**Step 1: Update main.rs — replace armed task spawning (lines 147-155)**

Replace this block:
```rust
    let armed_pin = esp_hal::gpio::Input::new(
        board.D2,
        esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::Up),
    );
    armed::init_armed_state(&armed_pin);
    spawner
        .spawn(armed::armed_monitor_task(armed_pin))
        .expect("Failed to spawn armed_monitor_task");
    info!("Armed monitor task spawned");
```

With:
```rust
    let armed_pin = esp_hal::gpio::Input::new(
        board.D2,
        esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::Up),
    );
    sequencer::init_armed_state(&armed_pin);
    let signal_light_i2c = acquire_i2c_bus();
    spawner
        .spawn(sequencer::state_sequencer_task(
            armed_pin,
            signal_light_i2c,
        ))
        .expect("Failed to spawn state_sequencer_task");
    info!("State sequencer task spawned");
```

**Step 2: Update mqtt/client.rs — StateCommandHandler**

Replace the `StateCommandHandler` impl (lines 90-103):
```rust
impl StateCommandHandler for AppCommandHandlers {
    fn handle_state_command(&mut self, command: StateCommand) {
        crate::sequencer::send_state_command(command);
        info!("MQTT command: state -> {:?}", command);
    }
}
```

**Step 3: Update mqtt/client.rs — remove state management from AppCommandHandlers**

In `AppCommandHandlers` struct (line 76-79), remove the `state` field:
```rust
struct AppCommandHandlers {
    shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>,
}
```

Update `new()` (lines 81-87):
```rust
impl AppCommandHandlers {
    fn new(shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>) -> Self {
        Self { shutdown_signal }
    }
}
```

**Step 4: Update ServoCommandHandler to read state from sequencer**

Replace the servo command handler (lines 105-118):
```rust
impl ServoCommandHandler for AppCommandHandlers {
    fn handle_servo_command(&mut self, command: ServoCommand) {
        if crate::sequencer::load_state() == StateStatus::Fire {
            warn!("MQTT command ignored: cmd/servo in FIRE state");
            queue::publish_command_log("Servo command rejected: FIRE state");
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

**Step 5: Update `publish_state_on_connect()` (line 195-199)**

Replace:
```rust
fn publish_state_on_connect() {
    let _ = queue::publish_state_status(load_state_status());
    crate::servo::republish_servo_state();
    crate::armed::republish_armed_state();
    queue::publish_command_log("Connected");
```

With:
```rust
fn publish_state_on_connect() {
    crate::sequencer::republish_sequencer_state();
    crate::servo::republish_servo_state();
    crate::sequencer::republish_armed_state();
    queue::publish_command_log("Connected");
```

**Step 6: Remove unused imports and functions from mqtt/client.rs**

Remove `store_state_status`, `load_state_status`, `CURRENT_STATE_STATUS` (lines 48-65). Remove the now-unused `StateStatus` import from the use block if no longer needed.

**Step 7: Verify it compiles**

Run: `cargo check --bin test_stand_controller`
Expected: errors about missing `state_sequencer_task` — that's the next task.

---

### Task 5: Implement the state sequencer task

**Files:**
- Modify: `src/bin/test_stand_controller/sequencer.rs` (add task function)

**Step 1: Add the `state_sequencer_task` function**

Append to `sequencer.rs`:

```rust
fn light_for_state(state: StateStatus) -> SignalLightConfig {
    match state {
        StateStatus::Armed => SignalLightConfig {
            green: true,
            ..SignalLightConfig::default()
        },
        StateStatus::Fire => SignalLightConfig {
            red: true,
            ..SignalLightConfig::default()
        },
        StateStatus::PostFire => SignalLightConfig {
            green: true,
            red: true,
            ..SignalLightConfig::default()
        },
    }
}

fn is_safety_armed(pin: &Input<'_>) -> bool {
    pin.is_high()
}

fn publish_armed_change(pin: &Input<'_>) {
    let value = pin.is_high() as u8;
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    let packet = ArmedPacket::new(timestamp_ms(), value);
    info!("Armed switch: {}", value);
    if crate::mqtt::publish_armed_sensor(packet).is_err() {
        warn!("Dropping armed packet: outbound queue full");
    }
}

fn transition_state(
    state: &mut StateStatus,
    new_state: StateStatus,
    light: &mut SignalLight<I2cType>,
) {
    *state = new_state;
    store_state(new_state);
    let _ = queue::publish_state_status(new_state);
    queue::publish_command_log(new_state.as_log());
    info!("State: {}", new_state.as_str());
    let config = light_for_state(new_state);
    if let Err(e) = light.set(config) {
        warn!("Failed to set signal light: {:?}", defmt::Debug2Format(&e));
    }
}

#[embassy_executor::task]
pub async fn state_sequencer_task(
    mut armed_pin: Input<'static>,
    signal_light_i2c: I2cType,
) {
    let address = pcf857x::SlaveAddr::Alternative(false, false, true);
    let mut light = match SignalLight::new(signal_light_i2c, address) {
        Ok(l) => l,
        Err(e) => {
            warn!(
                "Failed to initialize signal light: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut state = StateStatus::Armed;
    store_state(state);
    let config = light_for_state(state);
    if let Err(e) = light.set(config) {
        warn!("Failed to set initial light: {:?}", defmt::Debug2Format(&e));
    }
    info!("State sequencer initialized: ARMED");

    loop {
        match select(
            SEQUENCER_COMMAND_CHANNEL.receive(),
            armed_pin.wait_for_any_edge(),
        )
        .await
        {
            Either::First(command) => {
                handle_command(command, &mut state, &armed_pin, &mut light).await;
            }
            Either::Second(()) => {
                publish_armed_change(&armed_pin);
            }
        }
    }
}

async fn handle_command(
    command: StateCommand,
    state: &mut StateStatus,
    armed_pin: &Input<'_>,
    light: &mut SignalLight<I2cType>,
) {
    match command {
        StateCommand::Fire => {
            if *state != StateStatus::Armed {
                warn!("Ignoring FIRE: not in ARMED state");
                queue::publish_command_log("FIRE rejected: not ARMED");
                return;
            }
            if !is_safety_armed(armed_pin) {
                warn!("Ignoring FIRE: safety switch not armed");
                queue::publish_command_log("FIRE rejected: safety not armed");
                return;
            }

            // Transition to FIRE with buzzer sequence
            *state = StateStatus::Fire;
            store_state(StateStatus::Fire);
            let _ = queue::publish_state_status(StateStatus::Fire);
            queue::publish_command_log(StateStatus::Fire.as_log());
            info!("State: FIRE");

            // Phase 1: buzzer + red for 3 seconds
            let buzzer_config = SignalLightConfig {
                red: true,
                buzzer: true,
                ..SignalLightConfig::default()
            };
            if let Err(e) = light.set(buzzer_config) {
                warn!("Failed to set fire buzzer: {:?}", defmt::Debug2Format(&e));
            }

            Timer::after(Duration::from_millis(FIRE_BUZZER_DURATION_MS)).await;

            // Phase 2: red only (if still in FIRE state)
            if *state == StateStatus::Fire {
                let red_config = SignalLightConfig {
                    red: true,
                    ..SignalLightConfig::default()
                };
                if let Err(e) = light.set(red_config) {
                    warn!("Failed to set fire light: {:?}", defmt::Debug2Format(&e));
                }
            }
        }
        StateCommand::FireEnd => {
            if *state != StateStatus::Fire {
                warn!("Ignoring FIRE_END: not in FIRE state");
                queue::publish_command_log("FIRE_END rejected: not in FIRE");
                return;
            }
            transition_state(state, StateStatus::PostFire, light);
        }
        StateCommand::FireReset => {
            if *state != StateStatus::PostFire {
                warn!("Ignoring FIRE_RESET: not in POSTFIRE state");
                queue::publish_command_log("FIRE_RESET rejected: not in POSTFIRE");
                return;
            }
            transition_state(state, StateStatus::Armed, light);
        }
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check --bin test_stand_controller`
Expected: PASS

**Step 3: Commit**

```
feat: add signal light driver and state sequencer task

Replace armed monitor with state sequencer that controls
ARMED/FIRE/POSTFIRE transitions and PCF8574 signal light.
```

---

### Task 6: Delete armed.rs and clean up

**Files:**
- Delete: `src/bin/test_stand_controller/armed.rs`
- Verify: no remaining references to `armed::`

**Step 1: Delete `armed.rs`**

Remove the file entirely.

**Step 2: Verify clean compile**

Run: `cargo check --bin test_stand_controller`
Expected: PASS

**Step 3: Run clippy**

Run: `cargo clippy --bin test_stand_controller -- -D warnings`
Expected: PASS (no warnings)

**Step 4: Commit**

```
refactor: remove old armed.rs module
```

---

### Task 7: Update AGENTS.md and docs

**Files:**
- Modify: `AGENTS.md` (add signal_light module, update test_stand_controller description)

**Step 1: Add signal_light to module listing and update test_stand_controller entry**

Update the relevant sections to reflect:
- `src/signal_light.rs` — PCF8574-based signalling light driver
- `sequencer.rs` replaces `armed.rs` — state sequencer for ARMED/FIRE/POSTFIRE with signal light control
- State commands are now forwarded from MQTT to the sequencer via channel

**Step 2: Commit**

```
docs: update AGENTS.md with signal light and sequencer modules
```
