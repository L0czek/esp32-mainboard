# Repository Guidelines

## Project Structure & Module Organization
Current work scope includes `src/bin/test_stand_controller/` and `src/bin/tmp107_sensor_test/`.
- `scripts/send_shutdown_mqtt.sh`: helper script to publish MQTT shutdown command.
- `src/bin/adc_conversion_test/main.rs`: standalone ADC benchmark binary (A0 only, 1000 blocking one-shot conversions timed in microseconds).
- `scripts/publish_test_stand_elf.sh`: helper script to publish the matching
  `test_stand_controller` ELF as retained MQTT state for DEFMT decoders.
- `src/bin/test_stand_controller/main.rs`: boot path, task wiring, power + WiFi + MQTT startup.
- `src/idle_monitor.rs`: shared ESP RTOS idle hook + CPU utilization window tracker.
- `src/wifi.rs`: shared WiFi init/reconnect logic and STA RSSI watch updates.
- `src/bin/test_stand_controller/sensor_collection.rs`: ADC sensor collection task (100-sample fast batch + slow sweep).
- `src/bin/test_stand_controller/temperature_collection.rs`: TMP107 temperature collection task (20Hz one-shot + shutdown, 20-sample batched MQTT publish).
- `src/bin/tmp107_sensor_test/main.rs`: standalone TMP107 diagnostic binary (discover chain, one-shot read, log values, blink LED pattern).
- `src/bin/blackbox_uart_counter/main.rs`: standalone UART1 TX debug binary that sends incrementing `u32` values on blackbox pins.
- `src/bin/test_stand_controller/mqtt/client.rs`: DNS, TCP, MQTT session, command subscribe, event/select loop.
- `src/bin/test_stand_controller/mqtt/queue.rs`: global outbound queue and public publish API.
- `src/bin/test_stand_controller/mqtt/sensors/`: binary sensor/status payload types + encoders.
- `src/bin/test_stand_controller/mqtt/commands/`: command decoding + handler traits + mock handlers.
- `src/bin/test_stand_controller/mqtt/commands/shutdown.rs`: `SHUTDOWN` command decoder.
- `src/bin/test_stand_controller/mqtt/topics.rs`: topic constants and topic-format helpers.
- `src/bin/test_stand_controller/sequencer.rs`: state sequencer and armed-pin tasks (ARMED/FIRE/POSTFIRE state machine, fire trigger timing, signal light control, safety switch monitoring).
- `src/bin/test_stand_controller/servo.rs`: servo controller task (MCPWM PWM, command channel, linear interpolation).
- `src/bin/test_stand_controller/blackbox.rs`: UART1 blackbox data logger — streams sensor data to external recording device.
- `src/bin/test_stand_controller/config.rs`: compile-time env configuration (WiFi, MQTT, servo positions, blackbox baud rate).
- `src/tasks/` and `src/power/`: shared power-controller and interrupt handling used by this binary.
- `src/signal_light.rs`: PCF8574-based signalling light tower driver (active-low, 5 LEDs + buzzer).
- `src/tmp107/mod.rs`: TMP107 daisy-chain temperature sensor driver public API + UART protocol flow.
- `src/tmp107/registers.rs`: TMP107 register enum + configuration-register bitfield definition.
- `src/tmp107/commands.rs`: TMP107 command enum + command-byte encoding.

## Host Tools

### `tools/blackbox-decoder/` — Blackbox SD Card Tool
Standalone Rust crate (x86, stable toolchain) with two subcommands:
- **decode**: reads raw binary from SD card into NDJSON on stdout. Handles
  zero-padding (silently skipped), experiment separator bytes (emits
  `{"type":"experiment_separator"}`), and fails on unknown byte IDs.
- **format**: zero-fills an SD card or image file before a new experiment.
  Requires confirmation (`--yes` to skip). `--quick` stops at the first
  chunk that is already all zeros (fast re-format after a previous full format).

**Files:**
- `src/packet.rs`: Packet ID constants and `PacketData` enum (serde-tagged).
- `src/decoder.rs`: `PacketDecoder<R: Read>` — sequential binary reader with offset tracking.
- `src/main.rs`: CLI entry point (clap subcommands), decode + format logic.
- `.cargo/config.toml`: Overrides parent riscv target to x86_64.
- `rust-toolchain.toml`: Forces stable toolchain (parent uses nightly).
- `Makefile`: Build/check/fmt targets with `RUSTFLAGS=""` to clear parent's nightly flags.

**Build:** `cd tools/blackbox-decoder && make build`
**Lint:** `make check` (clippy) and `make fmt` (rustfmt)
**Decode:** `RUSTFLAGS="" cargo run -- decode [--separator <hex>] <path>`
**Format:** `RUSTFLAGS="" cargo run -- format [--yes] <device-or-file>`

### `tools/defmt-mqtt-decoder/` — Defmt MQTT Decoder
Standalone Rust crate (host + WASM-oriented) that reuses `defmt-decoder` in two modes:
- **CLI mode:** subscribes to MQTT `defmt` bytes and prints decoded log lines using the matching
  ELF.
- **WASM mode:** exposes a thin `wasm-bindgen` API for frontend code that already has MQTT payload
  bytes and wants decoded log lines without speaking MQTT directly.

**Files:**
- `src/decoder.rs`: transport-agnostic incremental decoder core. Accepts raw MQTT payload bytes
  and returns completed log lines plus recoverable warnings.
- `src/mqtt.rs`: host-only MQTT subscribe loop and payload forwarding.
- `src/main.rs`: CLI entry point that wires MQTT subscription into the shared decoder core.
- `src/wasm.rs`: WASM-facing `DefmtDecoder` wrapper for frontend integration.
- `example-web/`: tiny static website showing how browser JavaScript imports the generated WASM
  package and feeds raw MQTT payload bytes into the decoder.
- `tests/stream_decode.rs`: host-side MQTT event forwarding tests.
- `tests/defmt_ring.rs`: shared byte-ring regression tests for the firmware log transport.

**Build CLI:** `cd tools/defmt-mqtt-decoder && RUSTFLAGS="" cargo run -- --elf <firmware.elf> ...`
**Test/Lint:** `RUSTFLAGS="" cargo test && RUSTFLAGS="" cargo clippy --all-targets -- -D warnings`
**Build WASM:** `RUSTFLAGS="" cargo build --target wasm32-unknown-unknown --release --lib`
**Browser demo:** `RUSTFLAGS="" wasm-pack build --target web --out-dir example-web/pkg && cd example-web && python3 -m http.server 8080`

## Build, Test, and Development Commands
- `cargo check --bin test_stand_controller`: fast compile check with auto-loaded compile-time env.
- `cargo build --release --bin test_stand_controller`: optimized firmware build with auto-loaded compile-time env.
- `cargo fmt --all`: format code.
- `cargo clippy --bin test_stand_controller -- -D warnings`: lint this binary strictly.
- `cargo check --bin tmp107_sensor_test`: fast compile check for the standalone TMP107 test target.
- `cargo clippy --bin tmp107_sensor_test -- -D warnings`: lint the standalone TMP107 test target strictly.
- `cargo check --bin blackbox_uart_counter`: fast compile check for UART blackbox receiver debug target.
- `cargo check --bin adc_conversion_test`: fast compile check for ADC conversion benchmark target.
- `cargo espflash --release --bin test_stand_controller /dev/ttyUSB0`: flash device (update serial path).
- `cargo run --bin test_stand_controller`: probe-run style flash/run flow when configured.
- `cargo run --bin tmp107_sensor_test`: flash/run the standalone TMP107 diagnostic loop when configured.
- `cargo run --bin blackbox_uart_counter`: flash/run UART blackbox counter stream for receiver debug.
- `cargo run --bin adc_conversion_test`: flash/run ADC conversion benchmark on A0.

## Current Implementation Status (test_stand_controller)
- Boot sequence is implemented: RTT logging, Embassy runtime startup, heap allocation, board pin mapping, shared I2C bus init.
- Power stack is active via shared tasks: charger/expander setup, watchdog reset loop, interrupt-driven mode switching (Charging vs OTG), state watch channel, and boost/shipping-mode commands.
- WiFi is implemented in STA mode with DHCP and reconnect-on-disconnect behavior.
- MQTT is implemented with reconnect loop and modular client:
- global outbound queue (`embassy_sync::channel`, capacity 128) with enum messages for all sensor/status payloads.
- non-blocking enqueue API for data collection (`publish_fast_sensors`, `publish_slow_sensors`,
  `publish_temperature_sensor`, `publish_armed_sensor`).
- sensor collection task reads raw ADC values for A0/A1/A2 fast channels and A3/A4/BatVol/BoostVol
  slow channels. Each published sample is the arithmetic mean of
  `ADC_OVERSAMPLING_SAMPLES` one-shot reads (default `2`). It batches fast
  channels into 100 samples collected at 1ms spacing.
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
- State sequencer task manages ARMED/FIRE/POSTFIRE transitions with safety interlocks:
  - Receives `StateCommand` (Fire/FireEnd/FireReset) from MQTT handler via `Channel<CriticalSectionRawMutex, StateCommand, 4>`.
  - FIRE transition requires safety switch to be armed (GPIO21 high); rejected otherwise.
  - Signal light (PCF8574 at 0x21): green=ARMED, buzzer+red→red=FIRE, green+red=POSTFIRE.
  - Owns the fire trigger (PCF8574 at 0x20) and a 3-second non-blocking FIRE buzzer timer before trigger activation.
  - Dedicated armed-pin task monitors GPIO21 (D2) edge changes, updates cached armed state, and publishes via MQTT.
  - MQTT client delegates state management to sequencer channel (no inline state).
- Config is compile-time via env vars: required `WIFI_SSID`, `WIFI_PASSWORD`, `MQTT_HOST`; optional `MQTT_USER`, `MQTT_PASSWORD`, `MQTT_CLIENT_ID`.
- Additional compile-time constants in `config.rs` include `ADC_OVERSAMPLING_SAMPLES` (default `2`)
  for ADC sample averaging before MQTT/blackbox publish.
- `build.rs` auto-loads `.env` and forwards values as `cargo:rustc-env`; explicit shell env values override `.env`.
- `main.rs` now exits its runtime wait loop on a shutdown signal and executes shipping mode + deep sleep.
- CPU idle monitoring is implemented for all binaries (`empty`, `www_test`, `railclock`,
  `test_stand_controller`, `tmp107_sensor_test`, `blackbox_uart_counter`):
  - each binary starts RTOS with `esp_rtos::start_with_idle_hook(..., idle_monitor::idle_hook)`.
  - idle hook accumulates scheduler-idle (`WFI`) time using SYSTIMER unit0 ticks.
  - each binary has an `idle_metrics_task` that logs busy/idle percentages every 5 seconds.
- `test_stand_controller` publishes the latest idle metric via MQTT:
  - topic: `metric/cpu/idle` (retained).
  - payload format: raw little-endian `u16` idle permille (`0..=1000`).
- `test_stand_controller` publishes the latest STA RSSI metric via MQTT:
  - topic: `metric/wifi/rssi` (retained).
  - payload format: raw little-endian `i32` RSSI in dBm.
- The matching controller ELF can be published for browser-side DEFMT decoding with:
  `scripts/publish_test_stand_elf.sh`
  - default topic: `shared/firmware/test_stand_controller/elf`
  - payload: raw ELF bytes, retained
- TMP107 temperature sensor chain: auto-discovery at boot via Address Initialize,
  one-shot + shutdown mode at 20Hz (50ms interval) for best accuracy per datasheet.
  Each cycle: global one-shot trigger → 20ms conversion wait → global read.
  20 readings batched per sensor into a single TempPacket published once per second
  to `sensor/temp/{id}` MQTT topics. Config constants in `config.rs`:
  `TEMP_COLLECTION_INTERVAL_MS` (50), `TEMP_BATCH_SIZE` (20), `ONESHOT_CONVERSION_MS` (20).
  Connected via UART0 (GPIO16 TX, GPIO17 RX) with D0 (GPIO23) as half-duplex
  transceiver direction, driven by UART DTR in hardware RS485 mode.
- `tmp107_sensor_test` provides a standalone hardware test for that same UART0 TMP107 chain:
  one-shot conversion, per-sensor temperature logging, a walking ALERT1/ALERT2 blink pattern,
  address-bit LED display, then repeat.
- Blackbox UART data logger streams all sensor data over UART1 TX (D4) to
  an external recording device for disaster-proof data retention. Operates in
  parallel with MQTT — both receive the same data independently.
  - Packet format: `ID(1) + payload` with fixed or self-describing lengths.
  - Packet IDs: 0x06 Timing Sync (14B: `marker[7] + timestamp_ms:u32 + fast_interval_ms:u16`),
    0x01 Fast ADC (3ch, 7B), 0x02 Slow ADC (4ch, 9B), 0x03 Temperature
    (per-sensor, 4B: `sensor_id:u8 + raw:u16`), 0x04 Digital (2B),
    0x05 Servo (3B). All multi-byte values little-endian.
  - Timing Sync marker bytes (after ID): ASCII `TIMESYN`
    (`54 49 4D 45 53 59 4E`).
  - `sensor_collection_task` sends one Timing Sync packet at the start of each
    fast batch before the first Fast ADC packet. Other packet types carry no
    embedded timestamp and must be time-aligned from Timing Sync + fast cadence.
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
