# ESP32 Mainboard Development Notes

## Repository Structure

- `src/lib.rs`
  Root library module declarations shared by the firmware binaries.
- `src/defmt_ring.rs`
  Shared fixed-capacity byte ring. The `test_stand_controller` `defmt` logger uses it as the
  non-blocking bridge between interrupt-safe log emission and the async MQTT publishing task.
- `src/bin/test_stand_controller/main.rs`
  Boot path for the test-stand firmware. Initializes RTT, WiFi, MQTT, sensors, servo, sequencer,
  idle metrics, and now the `defmt` MQTT drain task.
- `src/bin/test_stand_controller/defmt_logger.rs`
  Owns the binary-local `#[defmt::global_logger]`.
  Responsibilities:
  - initialize RTT without `rtt_target::rtt_init_defmt!()`
  - mirror `rtt-target`'s encoder/acquire/release behavior closely
  - tee encoded `defmt` bytes to RTT and to the shared byte ring
  - drain ring bytes into MQTT log-chunk messages
- `src/bin/test_stand_controller/mqtt/queue.rs`
  MQTT queue layer. Operational telemetry/status messages and `defmt` log chunks use separate
  bounded queues so diagnostic traffic cannot consume the capacity reserved for normal publishes.
- `src/bin/test_stand_controller/mqtt/client.rs`
  MQTT session loop. Uses explicit priority order for session work:
  inbound MQTT traffic first, operational outbound messages second, and raw `defmt` log chunks on
  `log/defmt` last.
- `src/bin/test_stand_controller/mqtt/topics.rs`
  Topic constants, including `TOPIC_LOG_DEFMT`.
- `tools/blackbox-decoder/`
  Existing native host tool for the UART blackbox binary stream.
- `tools/defmt-mqtt-decoder/`
  Native host tool for live MQTT `defmt` logs.
  - `src/main.rs`: CLI entry point
  - `src/mqtt.rs`: MQTT subscription loop, broker credential handling, and event forwarding
  - `src/decoder.rs`: ELF loading and `defmt-decoder` stream processing
  - `tests/stream_decode.rs`: verifies MQTT event-to-payload forwarding
  - `tests/defmt_ring.rs`: host-side tests for the shared ring implementation via path include
  - `tests/fixtures/defmt-fixture/`: tiny host-side `defmt` emitter used for decoder smoke tests

## Defmt MQTT Logging Design

`defmt` allows only one global logger, so `test_stand_controller` cannot keep using
`rtt-target`'s private `#[global_logger]` once it needs MQTT transport. The firmware therefore owns
its own tee logger and follows `rtt-target`'s implementation closely:

- one global `defmt::Encoder`
- one acquire/release guard
- synchronization via `critical-section`
- best-effort writes only

The tee logger writes the same encoded byte slices to:

1. an RTT up-channel for existing probe/debug workflows
2. a fixed byte ring consumed by an async MQTT drain task

The MQTT side intentionally transports encoded bytes, not formatted text. Host-side decoding uses
the firmware ELF and `defmt-decoder`, which keeps the wire format faithful to normal `defmt`
transport expectations.

## Host Tool Build Notes

The repo root forces the firmware RISC-V target and nightly-only rustflags. Native host tools must
override the target locally and clear inherited rustflags when invoked. For
`tools/defmt-mqtt-decoder/`, use:

```sh
cd tools/defmt-mqtt-decoder
env RUSTFLAGS='' cargo check
env RUSTFLAGS='' cargo test
env RUSTFLAGS='' cargo clippy --all-targets -- -D warnings
```

## Verification Notes

Host-side automated verification covers:

- shared byte-ring behavior
- MQTT event payload forwarding
- decoder handling for split payloads and malformed-frame recovery
- decoder smoke coverage with a real fixture ELF and emitted `defmt` byte stream
- native build/test/lint of the decoder tool

Firmware-side automated verification covers:

- `cargo check --bin test_stand_controller`
- `cargo clippy --bin test_stand_controller -- -D warnings`

Live RTT + MQTT + decoder end-to-end verification still requires hardware and a running broker.
