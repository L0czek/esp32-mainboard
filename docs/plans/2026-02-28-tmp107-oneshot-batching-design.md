# TMP107 One-Shot + Shutdown Mode with Batched Reads

## Summary

Switch TMP107 temperature collection from continuous conversion to one-shot + shutdown mode for best accuracy (per datasheet recommendation). Increase read rate to 20Hz and batch 20 readings per sensor into a single TempPacket published once per second.

## Motivation

- Datasheet section 8.2.1.2.4 recommends shutdown + one-shot + global read for best accuracy
- Batching 20 readings per MQTT packet reduces publish overhead by 20x
- 20Hz provides better temporal resolution than 10Hz

## Design

### Driver (`src/tmp107.rs`)

New constants:
- `CONFIG_REGISTER: u8 = 0x01`
- Config register bit values: SD=bit11 (0x0800), OS=bit12 (0x1000)
- `CONFIG_SHUTDOWN: u16 = 0x0800` (SD=1 only)
- `CONFIG_ONESHOT: u16 = 0x1800` (SD=1 + OS=1)

New public methods:
- `shutdown()` — global write config register with SD=1
- `trigger_one_shot()` — global write config register with OS=1 + SD=1

Existing methods unchanged:
- `read_all_temperatures()` — global read temp register (already works)

`global_write` becomes actively used (remove `#[allow(dead_code)]`).

### Collection Task (`temperature_collection.rs`)

Flow per cycle (50ms interval, 20 times/sec):
1. `trigger_one_shot()` — global write to all sensors
2. `Timer::after_millis(20)` — wait for ADC conversion (datasheet: 12-15ms, TI recommends 20ms)
3. `read_all_temperatures()` — global read
4. Store readings in batch buffer at current sample index
5. When 20 samples collected: build one TempPacket per sensor, publish, reset batch

Batch buffer: `[[u16; TEMP_BATCH_SIZE]; MAX_SENSORS]` (20 samples x up to 32 sensors = 1280 bytes).

Timestamps: record `first_timestamp_ms` at sample 0, `last_timestamp_ms` at sample 19.

After init, call `shutdown()` once before entering the loop.

### Config (`config.rs`)

- `TEMP_COLLECTION_INTERVAL_MS: u64 = 50` (20Hz, was 100)
- `TEMP_BATCH_SIZE: usize = 20`
- `ONESHOT_CONVERSION_MS: u64 = 20`

### Timing Budget (per 50ms cycle)

| Phase | Duration |
|-------|----------|
| One-shot trigger (global write: 5 bytes @ 115200) | ~0.5ms |
| Conversion wait | 20ms |
| Global read TX (3 bytes) + RX (2 bytes x N sensors) | ~2-5ms |
| Total (N=5 sensors) | ~23ms |
| Margin | ~27ms |

## Files Changed

- `src/tmp107.rs` — add constants, `shutdown()`, `trigger_one_shot()`
- `src/bin/test_stand_controller/temperature_collection.rs` — rewrite loop for one-shot + batching
- `src/bin/test_stand_controller/config.rs` — update interval, add batch size and conversion wait constants
