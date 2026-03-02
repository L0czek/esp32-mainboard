# Blackbox UART Data Logger Design

## Purpose

Add a UART1-based blackbox data output to prevent complete data loss during
catastrophic events. The blackbox streams sensor data to an external recording
device over a unidirectional UART link. This operates in parallel with the
existing MQTT telemetry path — both receive the same data independently.

## Architecture

### Approach

UART1 TX driver owned by `sensor_collection_task`. A global
`embassy_sync::Channel` allows other tasks to enqueue packets. The sensor task
drains the channel non-blockingly every ~10ms.

### Why this approach

- ADC data is the most time-critical; writing from the same task context
  eliminates channel transit latency
- Blocking UART writes rely on the hardware FIFO (128 bytes on ESP32-C6),
  avoiding async overhead
- Single writer to UART1 eliminates contention

### Data flow

```
sensor_collection_task:
  owns UART1 TX (blocking)
  writes Fast/Slow ADC packets directly after each read
  drains BLACKBOX_CHANNEL every 10 fast iterations (~10ms)

temperature_collection_task --> BLACKBOX_CHANNEL --> sensor_task --> UART1
armed_monitor_task          --> BLACKBOX_CHANNEL --> sensor_task --> UART1
servo_controller_task       --> BLACKBOX_CHANNEL --> sensor_task --> UART1
any future task             --> BLACKBOX_CHANNEL --> sensor_task --> UART1
```

## Packet Format

All packets: `ID(1) + PAYLOAD(variable)`

- ID byte: identifies packet type; fixed or self-describing payload length
- PAYLOAD: raw little-endian binary, no delimiters, no checksum

### Packet Types

| ID   | Name           | Payload                                                              | Total bytes     |
|------|----------------|----------------------------------------------------------------------|-----------------|
| 0x01 | Fast ADC (3ch) | ts:u32 + tensometer:u16 + tank:u16 + combustion:u16                 | 11              |
| 0x02 | Slow ADC (4ch) | bat_stand:u16 + bat_comp:u16 + boost:u16 + starter:u16              | 9               |
| 0x03 | Temperature    | count:u8 + [raw:u16; count]                                         | 2 + 2*count     |
| 0x04 | Digital        | value:u8                                                            | 2               |
| 0x05 | Servo          | ticks:u16                                                           | 3               |

### Bandwidth analysis

At 921600 baud (~92 KB/s):
- Fast ADC at 1kHz: 11 bytes * 1000/s = 11 KB/s (12% of bandwidth)
- Slow ADC at ~10Hz: 9 * 10 = 90 B/s
- Temperature (4 sensors, 20Hz): 10 * 20 = 200 B/s
- Total steady-state: ~11.3 KB/s — well within capacity
- FIFO (128 bytes) can buffer ~11 fast ADC packets

### Decoder notes

- Read ID byte to determine payload structure:
  - IDs 0x01, 0x02, 0x04, 0x05: fixed-length payloads
  - ID 0x03: read count byte, then count * 2 bytes
- Unknown IDs: fail decoding (no resync byte in the stream)
- All multi-byte values are little-endian
- Fast ADC timestamps are u32 milliseconds since boot (wraps at ~49 days)

## Hardware Configuration

### Pin assignment

- **UART1 TX**: D3 / GPIO20 (defined in Board, currently unused)
- **UART1 RX**: not connected (TX-only, unidirectional)

### UART settings

- Baud rate: configurable via `BLACKBOX_BAUD_RATE` constant in `config.rs`
  (default: 921600)
- Mode: blocking, TX-only
- Framing: 8N1 (8 data bits, no parity, 1 stop bit)
- No flow control

## Module: `blackbox.rs`

New module in `src/bin/test_stand_controller/blackbox.rs` containing:

- Packet ID constants
- `BlackboxPacket` enum (for channel-transported packets):
  ```
  Temperature { count, values: [u16; MAX_SENSORS] }
  Digital { value }
  Servo { ticks }
  ```
- `BLACKBOX_CHANNEL: Channel<CriticalSectionRawMutex, BlackboxPacket, 32>`
- `send_to_blackbox(packet)` — public API, calls try_send, drops on full
- `BlackboxWriter` struct wrapping `UartTx<'static, Blocking>`:
  - `write_fast_adc(ts, tensometer, tank, combustion)`
  - `write_slow_adc(bat_stand, bat_comp, boost, starter)`
  - `write_packet(packet: &BlackboxPacket)`
  - Internal: serializes to stack buffer, writes to UART

## Integration Changes

### sensor_collection_task

- Accept additional IO: `UART1` peripheral + D3 pin
- Initialize `BlackboxWriter` at task start
- After each fast ADC triplet read: call `writer.write_fast_adc(...)`
- Every 10 fast iterations: drain `BLACKBOX_CHANNEL`, write each packet
- After slow ADC quad read: call `writer.write_slow_adc(...)`
- After slow reads: drain channel again

### temperature_collection_task

- After `driver.read_all_temperatures()`: construct
  `BlackboxPacket::Temperature` and call `send_to_blackbox()`
- One additional function call per 50ms tick

### armed_monitor_task

- After state change detection: call
  `send_to_blackbox(BlackboxPacket::Digital { ... })`
- One additional function call, only on change

### servo_controller_task

- After position update: call
  `send_to_blackbox(BlackboxPacket::Servo { ... })`
- One additional function call per servo event

### main.rs

- Pass `UART1` peripheral + `board.D3` to sensor_collection_task
- No changes to other task spawns

### board.rs

- Add `U1Tx: D3Pin` type alias (or reuse existing D3Pin)

### config.rs

- Add `BLACKBOX_BAUD_RATE: u32 = 921600` constant

## Existing Code Impact

- MQTT telemetry path: completely untouched
- Task timing: negligible impact (blocking UART writes to FIFO take
  microseconds for small packets)
- Memory: ~32 * sizeof(BlackboxPacket) for channel buffer + small stack
  buffers for serialization

## Extensibility

Any task can send arbitrary data via:
```rust
send_to_blackbox(BlackboxPacket::Log {
    len: data.len() as u8,
    data: padded_array,
});
```

New packet types: add a new ID constant and enum variant. The decoder skips
unknown IDs by scanning for the next SYNC byte.
