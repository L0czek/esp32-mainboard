# railclock — ESP32 Mainboard Firmware

Firmware for the Railclock mainboard (ESP32C6-based). This repository contains a small board-specific Rust library and a set of example/target binaries that exercise board features (GPIO, I²C, UART, power controller, network/web UI, etc.).

## Repository layout

- `Cargo.toml` — crate manifest and binaries (`www_test`, `empty`, `test_stand_controller`).
- `rust-toolchain.toml` — pinned Rust toolchain for the project.
- `scripts/` — helper scripts for common local workflows.
- `src/` — library and binary sources:
  - `board.rs` — board-specific wiring and helper functions.
  - `power/` — power controller driver and helpers.
  - `tasks/` — async tasks used by binaries (ADC, UART, digital IO, etc.).
  - `bin/` — firmware entrypoints:
    - `www_test/` — web server + diagnostic target (primary example).
    - `empty/` — minimal/empty binary.
    - `test_stand_controller/` — test stand firmware (power, WiFi, MQTT command + sensor pipeline).

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
WIFI_SSID=... WIFI_PASSWORD=... MQTT_HOST=... cargo build --release --bin test_stand_controller
```

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

## Using `www_test`

- The `www_test` target exposes a tiny web UI that lets you:
  - Toggle and read GPIOs
  - Interact with I²C devices (scan/read/write)
  - Send/receive raw UART data
  - Query and control the board power controller (battery/charger/12V boost status)

- Open the device web UI at the IP address shown on the serial console after boot.
