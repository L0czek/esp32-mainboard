# Signal Light Driver & State Sequencer

## Signal Light Driver (`src/signal_light.rs`)

New library-level driver for a PCF8574-based signalling light tower.

### Hardware mapping

All outputs are **active-low** (0 = on, 1 = off).

| Bit | Output   |
|-----|----------|
| P0  | green    |
| P1  | red      |
| P2  | yellow   |
| P3  | blue     |
| P4  | white    |
| P5  | buzzer   |
| P6  | reserved |
| P7  | reserved |

I2C address: `SlaveAddr::Alternative(false, false, true)` (0x21).

### API

```rust
pub struct SignalLightConfig {
    pub green: bool,
    pub red: bool,
    pub yellow: bool,
    pub blue: bool,
    pub white: bool,
    pub buzzer: bool,
}
```

- `SignalLight::new(i2c, address) -> Self` — wraps Pcf8574, initializes all off
- `set(&mut self, config: SignalLightConfig) -> Result<(), pcf857x::Error<E>>` — writes config as one byte
- `current(&self) -> SignalLightConfig` — returns last-set state (no I2C)

`SignalLightConfig` implements `Default` (all off) and conversion to/from `u8` with active-low inversion.

## State Sequencer (`src/bin/test_stand_controller/sequencer.rs`)

Replaces `armed.rs`. Owns the safety switch pin, signal light, and rocket state machine.

### Communication

- `static SEQUENCER_COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, StateCommand, 4>`
- Public `send_state_command(cmd)` function (non-blocking, like servo pattern)
- Existing `StateCommand` enum unchanged (Fire, FireEnd, FireReset)
- Existing `CURRENT_STATE_STATUS` atomic stays for read access; writes only from sequencer

### State machine

```
ARMED  --[Fire, safety armed]--> FIRE
ARMED  --[Fire, safety NOT armed]--> reject (stay ARMED)
FIRE   --[FireEnd]--> POSTFIRE
POSTFIRE --[FireReset]--> ARMED
```

### Signal light behavior

| State    | Lights              |
|----------|---------------------|
| ARMED    | green               |
| FIRE     | buzzer + red (3s), then red only |
| POSTFIRE | green + red         |

### Task structure

Uses `embassy_futures::select` to multiplex:
1. Command channel receive
2. Armed pin edge detection

On FIRE transition, a 3-second timer controls the buzzer-to-red-only switch.

### MQTT integration changes

- `mqtt/client.rs`: `StateCommandHandler` sends commands to sequencer channel instead of managing state directly
- `sequencer.rs` publishes state changes via existing `queue::publish_state_status()`
- Armed pin monitoring continues to publish via `mqtt::publish_armed_sensor()`
- `republish_armed_state()` and `republish_sequencer_state()` called on MQTT connect

### main.rs changes

- Replace `mod armed` with `mod sequencer`
- Pass armed pin + I2C bus to sequencer task
- Remove armed task spawn, add sequencer task spawn
