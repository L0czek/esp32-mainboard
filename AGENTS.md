# Repository Guidelines

## Project Structure & Module Organization
Current work scope is `src/bin/test_stand_controller/` only.
- `src/bin/test_stand_controller/main.rs`: boot path, task wiring, power + WiFi + MQTT startup.
- `src/bin/test_stand_controller/wifi.rs`: STA-mode WiFi init and reconnect loop.
- `src/bin/test_stand_controller/sensor_collection.rs`: ADC sensor collection task (100-sample fast batch + slow sweep).
- `src/bin/test_stand_controller/mqtt/client.rs`: DNS, TCP, MQTT session, command subscribe, event/select loop.
- `src/bin/test_stand_controller/mqtt/queue.rs`: global outbound queue and public publish API.
- `src/bin/test_stand_controller/mqtt/sensors/`: binary sensor/status payload types + encoders.
- `src/bin/test_stand_controller/mqtt/commands/`: command decoding + handler traits + mock handlers.
- `src/bin/test_stand_controller/mqtt/topics.rs`: topic constants and topic-format helpers.
- `src/bin/test_stand_controller/config.rs`: compile-time env configuration.
- `src/tasks/` and `src/power/`: shared power-controller and interrupt handling used by this binary.

## Build, Test, and Development Commands
- `WIFI_SSID=... WIFI_PASSWORD=... MQTT_HOST=... cargo check --bin test_stand_controller`: fast compile check.
- `WIFI_SSID=... WIFI_PASSWORD=... MQTT_HOST=... cargo build --release --bin test_stand_controller`: optimized firmware build.
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
- command subscribe/dispatch on `cmd/state` and `cmd/servo` with trait-based mock handlers.
- Config is compile-time via env vars: required `WIFI_SSID`, `WIFI_PASSWORD`, `MQTT_HOST`; optional `MQTT_USER`, `MQTT_PASSWORD`, `MQTT_CLIENT_ID`.
- Known gap: shutdown path in `main.rs` is unreachable due an infinite loop before the shutdown sequence.

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
