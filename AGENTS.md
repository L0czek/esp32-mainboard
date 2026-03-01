# Repository Guidelines

## Project Structure & Module Organization
Current work scope is `src/bin/test_stand_controller/` only.
- `scripts/send_shutdown_mqtt.sh`: helper script to publish MQTT shutdown command.
- `src/bin/test_stand_controller/main.rs`: boot path, task wiring, power + WiFi + MQTT startup.
- `src/bin/test_stand_controller/wifi.rs`: STA-mode WiFi init and reconnect loop.
- `src/bin/test_stand_controller/sensor_collection.rs`: ADC sensor collection task (100-sample fast batch + slow sweep).
- `src/bin/test_stand_controller/temperature_collection.rs`: TMP107 temperature collection task (20Hz one-shot + shutdown, 20-sample batched MQTT publish).
- `src/bin/test_stand_controller/mqtt/client.rs`: DNS, TCP, MQTT session, command subscribe, event/select loop.
- `src/bin/test_stand_controller/mqtt/queue.rs`: global outbound queue and public publish API.
- `src/bin/test_stand_controller/mqtt/sensors/`: binary sensor/status payload types + encoders.
- `src/bin/test_stand_controller/mqtt/commands/`: command decoding + handler traits + mock handlers.
- `src/bin/test_stand_controller/mqtt/commands/shutdown.rs`: `SHUTDOWN` command decoder.
- `src/bin/test_stand_controller/mqtt/topics.rs`: topic constants and topic-format helpers.
- `src/bin/test_stand_controller/servo.rs`: servo controller task (MCPWM PWM, command channel, linear interpolation).
- `src/bin/test_stand_controller/blackbox.rs`: UART1 blackbox data logger — streams sensor data to external recording device.
- `src/bin/test_stand_controller/config.rs`: compile-time env configuration (WiFi, MQTT, servo positions, blackbox baud rate).
- `src/tasks/` and `src/power/`: shared power-controller and interrupt handling used by this binary.
- `src/tmp107.rs`: TMP107 daisy-chain temperature sensor driver (SMAART wire protocol over half-duplex UART).

## Build, Test, and Development Commands
- `cargo check --bin test_stand_controller`: fast compile check with auto-loaded compile-time env.
- `cargo build --release --bin test_stand_controller`: optimized firmware build with auto-loaded compile-time env.
- `cargo fmt --all`: format code.
- `cargo clippy --bin test_stand_controller -- -D warnings`: lint this binary strictly.
- `cargo espflash --release --bin test_stand_controller /dev/ttyUSB0`: flash device (update serial path).
- `cargo run --bin test_stand_controller`: probe-run style flash/run flow when configured.

## Current Implementation Status (test_stand_controller)
- Boot sequence is implemented: RTT logging, Embassy runtime startup, heap allocation, board pin mapping, shared I2C bus init.
- Power stack is active via shared tasks: charger/expander setup, watchdog reset loop, interrupt-driven mode switching (Charging vs OTG), state watch channel, and boost/shipping-mode commands.
- WiFi is implemented in STA mode with DHCP and reconnect-on-disconnect behavior.
- MQTT is implemented with reconnect loop and modular client:
- global outbound queue (`embassy_sync::channel`, capacity 128) with enum messages for all sensor/status payloads.
- non-blocking enqueue API for data collection (`publish_fast_sensors`, `publish_slow_sensors`,
  `publish_temperature_sensor`, `publish_armed_sensor`).
- sensor collection task reads raw ADC values for A0/A1/A2 fast channels and A3/A4/BatVol/BoostVol
  slow channels. It batches fast channels into 100 samples collected at 1ms spacing.
- binary payload encoding for fast/slow ADC, armed digital stream, temperature streams, and servo sensor.
- command subscribe/dispatch on `cmd/state`, `cmd/servo`, and `cmd/shutdown` (`SHUTDOWN` payload)
  with trait-based handlers.
- Servo controller task drives a pulse-width servo on GPIO22 (D1) via MCPWM0:
  - 160 MHz peripheral clock, 50 Hz PWM (period 19999), up-counting active high.
  - Receives `ServoCommand` (Open/Close) from MQTT handler via `Channel<CriticalSectionRawMutex, ServoCommand, 4>`.
  - Linear interpolation at 20ms tick intervals; travel time proportional to distance.
  - Publishes `ServoStatus` (OPENING/CLOSING/OPEN/CLOSED) on transitions via outbound queue.
  - Publishes `ServoSensorPacket` (current PWM ticks) on each interpolation step.
  - Config constants: `SERVO_MIN_PULSE_TICKS` (0°), `SERVO_MAX_PULSE_TICKS` (180°), `SERVO_OPEN_DEGREES`, `SERVO_CLOSED_DEGREES`, `SERVO_FULL_RANGE_MS`.
  - Boots to closed position. Mid-movement commands restart interpolation from current position.
- Config is compile-time via env vars: required `WIFI_SSID`, `WIFI_PASSWORD`, `MQTT_HOST`; optional `MQTT_USER`, `MQTT_PASSWORD`, `MQTT_CLIENT_ID`.
- `build.rs` auto-loads `.env` and forwards values as `cargo:rustc-env`; explicit shell env values override `.env`.
- `main.rs` now exits its runtime wait loop on a shutdown signal and executes shipping mode + deep sleep.
- TMP107 temperature sensor chain: auto-discovery at boot via Address Initialize,
  one-shot + shutdown mode at 20Hz (50ms interval) for best accuracy per datasheet.
  Each cycle: global one-shot trigger → 20ms conversion wait → global read.
  20 readings batched per sensor into a single TempPacket published once per second
  to `sensor/temp/{id}` MQTT topics. Config constants in `config.rs`:
  `TEMP_COLLECTION_INTERVAL_MS` (50), `TEMP_BATCH_SIZE` (20), `ONESHOT_CONVERSION_MS` (20).
  Connected via UART0 (GPIO16 TX, GPIO17 RX) with D0 (GPIO23) as half-duplex
  transceiver direction, driven by UART DTR in hardware RS485 mode.
- Blackbox UART data logger streams all sensor data over UART1 TX (D3/GPIO20) to
  an external recording device for disaster-proof data retention. Operates in
  parallel with MQTT — both receive the same data independently.
  - Packet format: `ID(1) + payload` with fixed or self-describing lengths.
  - Packet IDs: 0x01 Fast ADC (3ch, 11B), 0x02 Slow ADC (4ch, 13B), 0x03 Temperature
    (variable, 6+2*count B), 0x04 Digital (6B), 0x05 Servo (7B). All multi-byte values
    little-endian.
  - UART1 TX owned by `sensor_collection_task` (blocking writes to hardware FIFO).
    Other tasks send via `BLACKBOX_CHANNEL` (embassy_sync Channel, capacity 32);
    sensor task drains it every ~10ms.
  - Configurable baud rate via `BLACKBOX_BAUD_RATE` in config.rs (default 921600).
  - Design doc: `docs/plans/2026-03-01-blackbox-uart-design.md`.

## Coding Style & Naming Conventions
Use `rustfmt` defaults (4-space indentation, standard brace style). Follow idiomatic Rust naming:
- `snake_case` for functions/modules/files.
- `PascalCase` for types/enums/traits.
- `SCREAMING_SNAKE_CASE` for constants.
Keep control flow explicit in hardware paths; avoid hidden side effects.

## Testing Guidelines
Host-side tests are minimal. For each change run `cargo check --bin test_stand_controller`, `cargo clippy`, and on-device smoke checks (boot, WiFi join, MQTT connect, subscribe/publish round-trip).

## Commit & Pull Request Guidelines
Recent history favors short imperative subjects (`Fix ...`, `Refactor ...`, `Add ...`). Keep commits single-purpose and readable. Avoid `WIP` commits in PRs.
PRs should include:
- What changed and which binary/module is affected.
- How it was verified (commands and hardware checks).
- Any config/environment assumptions (`.env` keys, serial port, board variant).

## Security & Configuration Tips
Use `.env.example` as the template for local configuration. Never commit secrets, device credentials, or private network details.
