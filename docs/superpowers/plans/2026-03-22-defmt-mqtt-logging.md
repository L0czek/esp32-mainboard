# Defmt MQTT Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `defmt` tee logger to `test_stand_controller` that sends encoded logs to RTT and MQTT, plus a host decoder tool that subscribes to MQTT and renders logs with the matching ELF.

**Architecture:** `test_stand_controller` will own the only `#[global_logger]` and implement a tee sink modeled closely on `rtt-target`'s `defmt` logger. The logger will fan out encoded bytes to RTT and to a fixed byte ring buffer drained by an async MQTT publisher task. A separate host-side Rust tool will subscribe to the log topic and decode the stream using `defmt-decoder`.

**Tech Stack:** Rust `no_std`, `defmt`, `critical-section`, `rtt-target`, Embassy tasks/channels, `defmt-decoder`, MQTT host client crate, `cargo check`, `cargo clippy`, `cargo test`

---

## File Structure

- Modify: `Cargo.toml`
  Add any target-side or host-tool dependencies required for the tee logger and decoder.
- Modify: `README.md`
  Document the MQTT log path and host decoder usage.
- Create: `DEV.md`
  Add repository technical notes and the new log transport/tool structure, since the repo currently lacks this file.
- Create: `src/defmt_ring.rs`
  Shared no-std fixed-capacity byte ring with host-runnable unit tests.
- Create: `src/bin/test_stand_controller/defmt_logger.rs`
  Binary-local tee logger, byte ring buffer, RTT init helper, and public API used by the MQTT drain task.
- Modify: `src/bin/test_stand_controller/main.rs`
  Register the new logger module, initialize RTT without `rtt_init_defmt!`, and spawn the MQTT log drain task.
- Modify: `src/bin/test_stand_controller/mqtt/topics.rs`
  Add the dedicated `defmt` log topic constant.
- Modify: `src/bin/test_stand_controller/mqtt/queue.rs`
  Add outbound message support for raw log chunks.
- Modify: `src/bin/test_stand_controller/mqtt/client.rs`
  Publish raw log chunk payloads to the dedicated log topic without retention.
- Create: `src/bin/test_stand_controller/defmt_logger_tests.rs` or inline unit tests in `defmt_logger.rs`
  Verify byte ring buffering, chunk draining, and drop behavior.
- Create: `tools/defmt-mqtt-decoder/Cargo.toml`
  Host-tool manifest.
- Create: `tools/defmt-mqtt-decoder/.cargo/config.toml`
  Override the repo's firmware target so the host tool builds for the native host.
- Create: `tools/defmt-mqtt-decoder/rust-toolchain.toml`
  Match the existing host-tool pattern and force a std-capable stable toolchain if needed.
- Create: `tools/defmt-mqtt-decoder/src/main.rs`
  CLI that subscribes to MQTT and forwards payloads into `defmt-decoder`.
- Create: `tools/defmt-mqtt-decoder/src/mqtt.rs`
  MQTT receive loop and connection handling for the host tool.
- Create: `tools/defmt-mqtt-decoder/src/decoder.rs`
  ELF loading, stream decoding, and formatted output helpers.
- Create: `tools/defmt-mqtt-decoder/tests/stream_decode.rs`
  Host-side tests for stream chunk boundaries and decoder behavior.

### Task 1: Establish Test/Build Baseline

**Files:**
- Modify: `.env` in the worktree only if needed for local build verification

- [ ] **Step 1: Confirm example env data is available**

Run: `sed -n '1,120p' .env.example`
Expected: `WIFI_SSID`, `WIFI_PASSWORD`, and `MQTT_HOST` example keys are present.

- [ ] **Step 2: Create a worktree-local `.env` from the example if missing**

Run: `test -f .env || cp .env.example .env`
Expected: `.env` exists in the worktree so compile-time env values resolve.

- [ ] **Step 3: Run the current firmware build check**

Run: `cargo check --bin test_stand_controller`
Expected: current baseline status is known before changes.

### Task 2: Add a Failing Test for the Logger Byte Ring

**Files:**
- Create: `src/defmt_ring.rs`

- [ ] **Step 1: Write the failing unit tests for the shared byte ring**

Add tests that cover:
- bytes are drained in FIFO order
- a drain can return a partial chunk and leave the remainder queued
- overflow drops new data instead of corrupting queued data

- [ ] **Step 2: Run the shared ring tests to verify they fail**

Run: `cargo test defmt_ring --lib`
Expected: FAIL because the ring implementation does not exist yet.

### Task 3: Implement the Tee Logger Core

**Files:**
- Modify: `src/lib.rs`
- Create: `src/defmt_ring.rs`
- Create: `src/bin/test_stand_controller/defmt_logger.rs`
- Modify: `src/bin/test_stand_controller/main.rs`

- [ ] **Step 1: Add the logger module skeleton**

Create:
- byte ring type with fixed capacity
- public `init_rtt_logger()` or equivalent helper
- public `take_log_chunk(...)` or equivalent drain API
- private `#[defmt::global_logger]` type

- [ ] **Step 2: Implement the ring minimally to satisfy the tests**

Keep the ring explicit and local; no generic abstraction beyond a single fixed-capacity shared
module.

- [ ] **Step 3: Run the shared ring tests to verify they pass**

Run: `cargo test defmt_ring --lib`
Expected: PASS

- [ ] **Step 4: Implement the `defmt::Logger` lifecycle**

Mirror `rtt-target` closely:
- `critical_section::acquire`
- taken flag
- global `defmt::Encoder`
- `start_frame`, `write`, `end_frame`

- [ ] **Step 5: Implement the tee sink callback**

On each encoded byte slice:
- write to the RTT up-channel
- append the same bytes to the MQTT log ring

- [ ] **Step 6: Replace `rtt_init_defmt!()` initialization in `main.rs`**

Initialize RTT channels manually and register the RTT up-channel with the local logger.

- [ ] **Step 7: Run the firmware build check**

Run: `cargo check --bin test_stand_controller`
Expected: PASS

### Task 4: Add MQTT Outbound Support for Raw Log Chunks

**Files:**
- Modify: `src/bin/test_stand_controller/mqtt/topics.rs`
- Modify: `src/bin/test_stand_controller/mqtt/queue.rs`
- Modify: `src/bin/test_stand_controller/mqtt/client.rs`

- [ ] **Step 1: Write a failing test for outbound log message chunk behavior if practical**

If the existing MQTT modules already have a unit-test pattern, add a focused test for:
- topic selection
- raw payload passthrough
- non-retained publish behavior

- [ ] **Step 2: Run the focused MQTT test to verify it fails**

Run: `cargo test mqtt --lib`
Expected: FAIL because log outbound support does not exist yet.

- [ ] **Step 3: Add the dedicated log topic constant**

Example: `TOPIC_LOG_DEFMT`.

- [ ] **Step 4: Add a new outbound message variant for raw log chunks**

Store borrowed-or-owned raw bytes in a way that fits the queue and current no-alloc patterns.

- [ ] **Step 5: Extend MQTT publish logic for the new variant**

Requirements:
- publish raw bytes unchanged
- QoS 0
- not retained

- [ ] **Step 6: Run the focused MQTT test**

Run: `cargo test mqtt --lib`
Expected: PASS

### Task 5: Add the Log Drain Task

**Files:**
- Modify: `src/defmt_ring.rs`
- Modify: `src/bin/test_stand_controller/defmt_logger.rs`
- Modify: `src/bin/test_stand_controller/main.rs`

- [ ] **Step 1: Write a failing shared test for bounded chunk draining**

Cover:
- drain returns no message when ring is empty
- drain produces bounded chunk sizes for MQTT publication

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `cargo test defmt_ring --lib`
Expected: FAIL because drain-to-MQTT plumbing is incomplete.

- [ ] **Step 3: Implement the async drain task**

The task should:
- poll the ring periodically or opportunistically
- publish available chunks through the existing MQTT outbound queue
- avoid blocking the logger path

- [ ] **Step 4: Spawn the drain task from `main.rs`**

Ensure it starts after logger/RTT initialization and before normal runtime logging becomes heavy.

- [ ] **Step 5: Run the focused test and build check**

Run: `cargo test defmt_ring --lib`
Expected: PASS

Run: `cargo check --bin test_stand_controller`
Expected: PASS

### Task 6: Add the Host Decoder Tool Tests First

**Files:**
- Create: `tools/defmt-mqtt-decoder/Cargo.toml`
- Create: `tools/defmt-mqtt-decoder/.cargo/config.toml`
- Create: `tools/defmt-mqtt-decoder/rust-toolchain.toml`
- Create: `tools/defmt-mqtt-decoder/src/decoder.rs`
- Create: `tools/defmt-mqtt-decoder/tests/stream_decode.rs`

- [ ] **Step 1: Scaffold the host tool crate**

Create the crate structure, native target override, and test target.

- [ ] **Step 2: Write failing host-side tests for stream feeding**

Cover:
- multiple MQTT payloads forming one continuous stream
- split-frame delivery across payloads
- malformed stream handling aligns with `defmt-decoder`

- [ ] **Step 3: Run the host tool tests to verify they fail**

Run: `cargo test --manifest-path tools/defmt-mqtt-decoder/Cargo.toml`
Expected: FAIL because the decoder helpers do not exist yet.

### Task 7: Implement the Host Decoder Tool

**Files:**
- Create: `tools/defmt-mqtt-decoder/src/main.rs`
- Create: `tools/defmt-mqtt-decoder/src/mqtt.rs`
- Create: `tools/defmt-mqtt-decoder/src/decoder.rs`

- [ ] **Step 1: Implement ELF loading and table parsing**

Use `defmt_decoder::Table::parse` and fail fast if `.defmt` metadata is missing or incompatible.

- [ ] **Step 2: Implement stream-decoder glue**

Use `table.new_stream_decoder()` and feed bytes exactly as they arrive from MQTT.

- [ ] **Step 3: Implement output formatting**

Follow `defmt-print` closely instead of inventing a new formatter.

- [ ] **Step 4: Implement MQTT subscribe loop**

Add CLI configuration for:
- broker host/port
- topic
- ELF path

- [ ] **Step 5: Run the host tool tests**

Run: `cargo test --manifest-path tools/defmt-mqtt-decoder/Cargo.toml`
Expected: PASS

- [ ] **Step 6: Run host tool lint/build verification**

Run: `cargo check --manifest-path tools/defmt-mqtt-decoder/Cargo.toml`
Expected: PASS

### Task 8: Update Documentation

**Files:**
- Modify: `README.md`
- Create: `DEV.md`

- [ ] **Step 1: Update `README.md`**

Document:
- new MQTT log topic
- host decoder usage
- requirement to use the matching ELF

- [ ] **Step 2: Add `DEV.md`**

Document:
- logger module responsibility
- log ring + drain task
- host decoder tool layout

- [ ] **Step 3: Verify docs reference the implemented behavior only**

Re-read `README.md` and `DEV.md`
Expected: no phantom features, no undocumented new behavior.

### Task 9: Final Verification

**Files:**
- Verify all touched files

- [ ] **Step 1: Run firmware unit tests**

Run: `cargo test defmt_ring --lib`
Expected: PASS

- [ ] **Step 2: Run firmware build verification**

Run: `cargo check --bin test_stand_controller`
Expected: PASS

- [ ] **Step 3: Run firmware lint verification**

Run: `cargo clippy --bin test_stand_controller -- -D warnings`
Expected: PASS

- [ ] **Step 4: Run formatting verification**

Run: `cargo fmt --all --check`
Expected: PASS

- [ ] **Step 5: Run host tool tests**

Run: `cargo test --manifest-path tools/defmt-mqtt-decoder/Cargo.toml`
Expected: PASS

- [ ] **Step 6: Run host tool lint/build verification**

Run: `cargo clippy --manifest-path tools/defmt-mqtt-decoder/Cargo.toml --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 7: Record end-to-end smoke status**

Verify or explicitly note status for:
- RTT still prints logs
- MQTT publishes encoded `defmt` bytes
- host tool decodes the live stream with the matching ELF

Expected: either verified on hardware/broker or clearly reported as not run in this session.

- [ ] **Step 8: Inspect `git diff --stat`**

Run: `git diff --stat`
Expected: only planned files changed.

## Post-Review Follow-Up TODOs

These items were identified during post-implementation review and are intentionally tracked here so
they are not lost.

- [x] **TODO: Isolate `defmt` log traffic from operational MQTT traffic**

Implemented:
- `defmt` log chunks now use a dedicated bounded queue separate from telemetry/status messages.
- the MQTT session loop now prioritizes work as inbound MQTT traffic, then operational
  telemetry/status, then `defmt` logs.
- `defmt` logs stay best effort and cannot consume the capacity reserved for normal publications.

- [x] **TODO: Add MQTT authentication support to the host decoder**

Implemented:
- `tools/defmt-mqtt-decoder` now accepts `--username` / `--password`
- the same values can also come from `MQTT_USER` / `MQTT_PASSWORD`
- the MQTT subscriber applies credentials when both values are present

- [x] **TODO: Strengthen end-to-end decode verification**

Implemented:
- split-payload handling and malformed-frame recovery remain covered by host decoder unit tests
- the broker-free fixture smoke test now feeds real emitted `defmt` bytes into the real host
  decoder and asserts the decoded output line content and level
- this closes the host-side verification gap without adding broker or hardware dependencies

Still not run:
- live hardware + broker smoke verification for the full firmware publisher path
