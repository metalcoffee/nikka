use sentinel_frame::with_sentinel_frame;

#[with_sentinel_frame]
pub extern "C" fn run_c(
    _a: i32,
    _b: *mut i32,
) {
}

fn main() {
}
