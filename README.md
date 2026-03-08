# railclock — ESP32 Mainboard Firmware

Firmware for the Railclock mainboard (ESP32C6-based). This repository contains a small board-specific Rust library and a set of example/target binaries that exercise board features (GPIO, I²C, UART, power controller, network/web UI, etc.).

## Repository layout

- `Cargo.toml` — crate manifest and binaries (`www_test`, `empty`, `test_stand_controller`, `tmp107_sensor_test`, `blackbox_uart_counter`).
- `rust-toolchain.toml` — pinned Rust toolchain for the project.
- `scripts/` — helper scripts for common local workflows.
- `src/` — library and binary sources:
  - `board.rs` — board-specific wiring and helper functions.
  - `power/` — power controller driver and helpers.
  - `tasks/` — async tasks used by binaries (ADC, UART, digital IO, etc.).
  - `tmp107/` — TMP107 UART daisy-chain driver split into protocol commands/registers and driver logic.
  - `bin/` — firmware entrypoints:
    - `www_test/` — web server + diagnostic target (primary example).
    - `empty/` — minimal/empty binary.
    - `test_stand_controller/` — test stand firmware (power, WiFi, MQTT command + sensor pipeline).
    - `tmp107_sensor_test/` — standalone TMP107 chain test (discover, read, log, LED blink loop).
    - `blackbox_uart_counter/` — UART1 (D4 TX) counter generator for blackbox receiver debugging.

## What this repo provides

This repository provides board support code for the Railclock mainboard — including power-controller drivers, GPIO mappings, and I²C helpers — together with an example firmware, `www_test`, which runs a small web UI to control GPIOs, interact with I²C and UART devices, and query/control the power controller (battery, 12V boost, charger). The crate also supplies low-level drivers and an async task layout built on Embassy to simplify integrating board features into firmware targets.

## Build

Build the `www_test` binary (example):

```sh
cargo build --release --bin www_test
```

To build the minimal `empty` binary:

```sh
cargo build --release --bin empty
```

To build the `test_stand_controller` binary:

```sh
cp .env.example .env
cargo build --release --bin test_stand_controller
```

To build the TMP107 sensor test binary:

```sh
cargo build --release --bin tmp107_sensor_test
```

To build the blackbox UART counter debug binary:

```sh
cargo build --release --bin blackbox_uart_counter
```

`build.rs` auto-loads `.env` at compile time for any `env!` config values.

## Flashing / Running


Example using `cargo-espflash` (replace `/dev/ttyUSB0` with your serial device):

```sh
cargo install cargo-espflash         # if not installed
cargo espflash --release --bin www_test /dev/ttyUSB0
```

`cargo espflash` will flash and typically open the serial monitor. Alternatively use `espflash`, `esptool.py`, or your preferred flashing tool.

Using probe-rs / `probe-run` (common workflow):

If you use probe-rs / `probe-run` to flash via a debug probe you can build, flash and run the target directly with `cargo run` (this matches your workflow):

```sh
cargo run --bin www_test            # or add --release for an optimized build
```

After the device boots, the `www_test` firmware runs a small web server and prints network/diagnostic info to the serial console (watch the serial log to discover the device IP or status messages).

## MQTT in `test_stand_controller`

- MQTT code is split under `src/bin/test_stand_controller/mqtt/`:
  - `client.rs` — connection/session loop with `select` over inbound MQTT events and outbound queue.
  - `queue.rs` — global outbound queue (capacity 128) and enqueue API.
  - `sensors/` — raw binary packet models + encoders for fast/slow sensors and statuses.
  - `commands/` — command decoders (`cmd/state`, `cmd/servo`, `cmd/shutdown`) and handlers.
  - `topics.rs` — prefixed topic constants (`...`) and topic utilities.
- `cmd/shutdown` accepts payload `SHUTDOWN` and triggers shipping-mode + deep-sleep shutdown.
- Helper script to send the shutdown command:
```sh
MQTT_HOST=broker.local MQTT_PORT=1883 scripts/send_shutdown_mqtt.sh
```
- Data collection integration entrypoints:
  - `publish_fast_sensors(...)`
  - `publish_slow_sensors(...)`
  - `publish_temperature_sensor(...)`
  - `publish_armed_sensor(...)`
- Sensor collection runtime:
  - `sensor_collection_task` reads raw ADC values.
  - Fast channels (A0/A1/A2) are batched into 100 samples with 1ms spacing per sample.
  - Slow channels (A3/A4/BatVol/BoostVol) are read once per cycle and enqueued without batching.
- `temperature_collection_task` polls the TMP107 UART chain on UART0, using hardware RS485
  direction control via D0 wired to UART DTR.

## Blackbox Stream (`test_stand_controller`)

- UART1 blackbox output uses compact binary packets (`ID + payload`) for offline capture.
- Packet timestamps are centralized in a dedicated Timing Sync packet (`0x06`) that is emitted
  once at the start of each fast ADC batch.
- Timing Sync packet layout is: `ID (0x06) + marker[7] + timestamp_ms:u32 + fast_interval_ms:u16`.
- Timing Sync marker bytes are fixed ASCII: `TIMESYN` (`54 49 4D 45 53 59 4E`).
- Fast ADC packets (`0x01`) are emitted at a fixed 1 ms cadence after each Timing Sync packet.
- Slow ADC (`0x02`), temperature (`0x03`), digital (`0x04`), and servo (`0x05`) packets carry
  values only (no embedded per-packet timestamp).
- `tools/blackbox-decoder` decodes this stream into NDJSON, including `timing_sync` events.

### UART Receiver Debug Target

- `blackbox_uart_counter` transmits incrementing `u32` values (little-endian) over
  the same blackbox interface: UART1 TX on `D4` at `3_000_000` baud.
- Use it to validate external UART receiver wiring and framing without the rest
  of the test stand pipeline.
- Run with:

```sh
cargo run --bin blackbox_uart_counter
```

## TMP107 Sensor Test

- `tmp107_sensor_test` is a dedicated diagnostic binary for the TMP107 daisy chain on UART0.
- On each cycle it:
  - triggers a one-shot conversion,
  - reads and logs every discovered sensor temperature,
  - blinks a simple LED pattern across ALERT1/ALERT2,
  - shows the address-bit LED pattern,
  - then repeats.
- Build or run it with:

```sh
cargo run --bin tmp107_sensor_test
```

## Using `www_test`

- The `www_test` target exposes a tiny web UI that lets you:
  - Toggle and read GPIOs
  - Interact with I²C devices (scan/read/write)
  - Send/receive raw UART data
  - Query and control the board power controller (battery/charger/12V boost status)

- Open the device web UI at the IP address shown on the serial console after boot.
