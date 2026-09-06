# Defmt MQTT Logging Design

## Goal

Add `defmt` log transport over MQTT for `test_stand_controller` while preserving RTT output.
The implementation must provide:

1. A `defmt` logger used by `test_stand_controller` that tees encoded `defmt` bytes to RTT and
   MQTT.
2. A host-side decoder tool that receives those MQTT log bytes and formats them using the firmware
   ELF metadata.

## Constraints

- `defmt` permits only one `#[global_logger]` in the dependency graph.
- `rtt-target` already provides a private `#[global_logger]`, so `test_stand_controller` cannot use
  it unchanged once a custom logger is introduced.
- The custom logger must follow the `defmt::Logger` contract:
  - `write` receives unencoded frame bytes.
  - the logger must own a `defmt::Encoder`.
  - logging must be best-effort and must not fail.
- RTT panic logging should remain in place through existing libraries where possible.
- The MQTT transport must carry raw encoded `defmt` bytes, not formatted text.

## Official `defmt` Model

The implementation follows the documented `defmt` data flow:

- `Logger::acquire` starts an encoded frame.
- `Logger::write` receives one or more raw frame fragments and pushes them through
  `defmt::Encoder`.
- `Logger::release` finishes the encoded frame.
- Host-side formatting is performed with `defmt-decoder` using `.defmt` metadata from the ELF.

This means the device must transmit encoded `defmt` stream bytes and the host must decode them with
the matching ELF.

## Recommended Architecture

### Device-side tee logger

`test_stand_controller` will own a binary-local logger module that closely follows
`rtt-target`'s `defmt` logger implementation:

- one global `defmt::Encoder`
- one global acquire/release guard
- synchronization through `critical-section`
- direct fan-out of encoded bytes from the encoder sink callback

The sink callback will write each encoded chunk to:

- an RTT up-channel
- a fixed-capacity byte ring used by the MQTT path

The logger remains best-effort:

- RTT uses non-blocking semantics matching current behavior as closely as practical.
- the MQTT ring drops new data when full instead of blocking.
- no sink may panic or return errors during logging.

### MQTT drain path

The MQTT side does not perform formatting or frame reconstruction. A dedicated async task drains
available bytes from the log ring and publishes them as binary MQTT payloads on a dedicated topic.

Properties:

- topic is dedicated to `defmt` logs, for example `log/defmt`
- QoS 0
- not retained
- payloads preserve byte order exactly as emitted by the logger
- a single MQTT message may contain several full frames, a partial tail, or both

This keeps the logger independent from network state and lets the host treat MQTT payloads as a
continuous `defmt` byte stream.

### Host decoder tool

A host-side Rust CLI under `tools/` will:

- load the firmware ELF
- parse `.defmt` metadata via `defmt_decoder::Table::parse`
- create `table.new_stream_decoder()`
- subscribe to the configured MQTT topic
- feed each MQTT payload into `StreamDecoder::received`
- decode and print frames using `defmt-decoder` formatting helpers

The tool is intentionally live-stream only in the first version:

- no persistence
- no replay
- no broker history assumptions

If the incoming stream and ELF are incompatible, the tool must fail fast.

## Repository Changes

### Firmware changes

- add a new `defmt` tee logger module under `src/bin/test_stand_controller/`
- initialize RTT manually without using `rtt_init_defmt!`, so the binary can own the only global
  logger
- add a byte ring buffer for MQTT log transport
- add an async drain task that publishes queued log bytes
- add a new MQTT outbound message variant and log topic handling
- wire logger and drain task into `test_stand_controller/main.rs`

### Host tooling changes

- add a new host tool crate under `tools/` for MQTT-based `defmt` decoding
- reuse `defmt-decoder` and formatting patterns from `defmt-print`
- document usage in `README.md`
- update developer-facing structure notes in `DEV.md`

## Failure and Drop Behavior

- If RTT is not attached or the RTT buffer is full, behavior should stay aligned with current
  non-blocking semantics.
- If the MQTT log ring is full, new log bytes are dropped.
- If MQTT is disconnected, the drain task cannot publish and queued logs may be dropped according
  to queue/ring pressure.
- Sensor/status publishing must remain independent from log publishing.

## Testing and Verification

### Firmware

- add focused tests for the MQTT log ring / chunking behavior where host-side testing is practical
- run `cargo check --bin test_stand_controller`
- run `cargo clippy --bin test_stand_controller -- -D warnings`

### Host tool

- add unit tests for stream feeding and decoding boundaries
- verify that partial-frame MQTT chunking still decodes correctly once the full stream arrives
- run tool-specific checks, tests, formatting, and clippy

### End-to-end smoke path

- build `test_stand_controller`
- run firmware with live MQTT broker
- confirm RTT output still works
- confirm `log/defmt` bytes appear on MQTT
- run the host decoder against the same ELF and confirm readable logs are produced

## Non-goals

- decoding on-device
- sending formatted text instead of `defmt` bytes
- broker-side persistence or replay
- changes to other binaries unless required by shared dependency wiring

## Implementation Notes

- Follow `rtt-target` logger behavior closely rather than inventing a different concurrency model.
- Reuse existing crates and APIs where possible:
  - `critical-section`
  - `defmt`
  - `rtt-target`
  - `defmt-decoder`
- Keep the design explicit and local to `test_stand_controller`; avoid unnecessary abstraction into
  shared modules until there is a second real consumer.
