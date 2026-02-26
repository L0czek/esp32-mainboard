# Servo Controller Task Design

## Context

The test stand controller needs to drive a pulse-width-controlled servo on
the D1 pin (GPIO22) via MCPWM. MQTT commands (OPEN/CLOSE) trigger slow,
linear movement between configurable positions. Current position is
published as telemetry.

## MCPWM Configuration

- **Clock:** PLL_F160M (160 MHz) peripheral clock. `timer_clock_with_frequency`
  computes prescaler and period for 50 Hz output.
- **Peripheral:** MCPWM0, operator0, timer0, pin A on GPIO22 (D1).
- **Mode:** Up-counting, active high.

## Configuration (config.rs)

```rust
pub const SERVO_MIN_PULSE_TICKS: u16 = 500;   // 0 degrees
pub const SERVO_MAX_PULSE_TICKS: u16 = 2500;  // 180 degrees
pub const SERVO_OPEN_DEGREES: u16 = 90;
pub const SERVO_CLOSED_DEGREES: u16 = 0;
pub const SERVO_FULL_RANGE_MS: u64 = 5000;
```

Degrees-to-ticks conversion:

    timestamp = MIN_PULSE + (degrees * (MAX_PULSE - MIN_PULSE)) / 180

Travel time is proportional:

    move_time = FULL_RANGE_MS * |open_deg - close_deg| / 180

## Architecture

### Approach: Dedicated Embassy task with command channel

A `servo_controller_task` Embassy task owns the MCPWM peripheral and receives
`ServoCommand` values from a `Channel<CriticalSectionRawMutex, ServoCommand, 4>`.

### State machine

```
Boot -> drive to CLOSED_DEGREES
          |
     +----v----+
     |  Idle   |<---------------------+
     |(waiting)|                       |
     +----+----+                       |
    recv Open/Close           reached target
          |                            |
     +----v----+              +--------+---+
     | Moving  |----step----->|  publish   |
     |(ticking)|<-------------|  position  |
     +---------+              +------------+
```

### Event loop

Uses `embassy_futures::select`:

- **Idle:** Await on the command channel (blocks until command arrives).
- **Moving:** Select between command channel and a tick timer (~20ms steps).
  - On tick: step current position toward target, call `set_timestamp`,
    publish `ServoSensorPacket`.
  - On command while moving: update target (allows reversing mid-move).
  - On reaching target: publish final `ServoStatus` (Open/Closed),
    return to Idle.

### Publishing

- `ServoStatus` (OPENING/CLOSING) on transition start.
- `ServoStatus` (OPEN/CLOSED) on reaching target.
- `ServoSensorPacket` with current tick value on each interpolation step.

### Step calculation

Given ~20ms tick interval:

    total_steps = move_time_ms / 20
    ticks_per_step = |target_ticks - current_ticks| / total_steps

## Wiring

### Command channel (servo.rs)

```rust
static SERVO_COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, ServoCommand, 4> = Channel::new();
```

Public `send_servo_command(cmd)` function calls `try_send`.

### MQTT handler (client.rs)

`AppCommandHandlers::handle_servo_command` sends to the channel instead of
logging (still guarded by fire-state check).

### main.rs

Pass `peripherals.MCPWM0` and `board.D1` to the servo task. Spawn alongside
existing tasks.

## Error handling

- **Command during movement:** Updates target, recalculates from current
  position. Smooth reversal, no jumps.
- **Duplicate commands:** Ignored if already at target.
- **FIRE state:** Blocked upstream in MQTT handler.
- **Queue full on publish:** Log warning, don't block servo movement.
- **MCPWM init failure:** Panic on boot.

## Files changed

| File | Change |
|------|--------|
| `config.rs` | Add 5 servo constants |
| `servo.rs` | New: MCPWM init, channel, interpolation, task |
| `main.rs` | Add `mod servo`, spawn task with MCPWM0 + D1 |
| `client.rs` | `handle_servo_command` sends to channel |
