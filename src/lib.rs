#![no_std]

pub mod board;
pub mod channel;
pub mod config;
pub mod defmt_ring;
pub mod fire_trigger;
pub mod idle_monitor;
pub mod power;
pub mod signal_light;
pub mod tasks;
pub mod tmp107;
pub mod wifi;

pub use board::I2cType;

// esp-rtos's `esp-radio` feature is enabled package-wide, so the precompiled
// `libesp-radio.a` blob is linked into every binary. On RISC-V chips its NVS
// objects reference `misc_nvs_init`/`misc_nvs_deinit`, which the C6 linker
// script (`esp32c6_provides.x`) aliases to these `__esp_radio_misc_nvs_*`
// symbols. esp-radio only defines them under `#[cfg(xtensa)]`, so on the C6
// they are undefined and any binary that doesn't pull in the full Wi-Fi path
// (e.g. the test/benchmark tools) fails to link. These stubs provide them; the
// radio is never started in those binaries, so they are never actually called.
// Note that in those binaries the stubs shadow the real `misc_nvs_*` from
// `libcore.a`. Fixed upstream in esp-radio 0.18.0 (esp-rs/esp-hal PR #4513);
// see the TODO in README.md.
#[cfg(target_arch = "riscv32")]
#[no_mangle]
unsafe extern "C" fn __esp_radio_misc_nvs_init() -> i32 {
    0
}

#[cfg(target_arch = "riscv32")]
#[no_mangle]
unsafe extern "C" fn __esp_radio_misc_nvs_deinit() {}
