# railclock — ESP32 Mainboard Firmware

Firmware for the Railclock mainboard (ESP32C6-based). This repository contains a small board-specific Rust library and a set of example/target binaries that exercise board features (GPIO, I²C, UART, power controller, network/web UI, etc.).

## Repository layout

- `Cargo.toml` — crate manifest and binaries (`www_test`, `empty`).
- `rust-toolchain.toml` — pinned Rust toolchain for the project.
- `src/` — library and binary sources:
  - `board.rs` — board-specific wiring and helper functions.
  - `power/` — power controller driver and helpers.
  - `tasks/` — async tasks used by binaries (ADC, UART, digital IO, etc.).
  - `bin/` — firmware entrypoints:
    - `www_test/` — web server + diagnostic target (primary example).
    - `empty/` — minimal/empty binary.

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

## Using `www_test`

- The `www_test` target exposes a tiny web UI that lets you:
  - Toggle and read GPIOs
  - Interact with I²C devices (scan/read/write)
  - Send/receive raw UART data
  - Query and control the board power controller (battery/charger/12V boost status)

- Open the device web UI at the IP address shown on the serial console after boot.

