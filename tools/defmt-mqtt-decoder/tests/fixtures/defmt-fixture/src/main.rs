use std::io::Write;

static mut ENCODER: defmt::Encoder = defmt::Encoder::new();

fn main() {
    defmt::info!("fixture={=u8}", 42);
}

#[defmt::global_logger]
struct Logger;

unsafe impl defmt::Logger for Logger {
    fn acquire() {
        unsafe { (*core::ptr::addr_of_mut!(ENCODER)).start_frame(write_stdout) };
    }

    unsafe fn flush() {}

    unsafe fn release() {
        (*core::ptr::addr_of_mut!(ENCODER)).end_frame(write_stdout);
    }

    unsafe fn write(bytes: &[u8]) {
        (*core::ptr::addr_of_mut!(ENCODER)).write(bytes, write_stdout);
    }
}

fn write_stdout(bytes: &[u8]) {
    std::io::stdout().write_all(bytes).unwrap();
}

#[no_mangle]
fn _defmt_timestamp(_: defmt::Formatter<'_>) {}
