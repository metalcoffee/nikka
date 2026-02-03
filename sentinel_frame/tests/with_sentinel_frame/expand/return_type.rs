use sentinel_frame::with_sentinel_frame;

#[with_sentinel_frame]
pub fn default() {
}

#[with_sentinel_frame]
pub(super) fn explicit_singleton() -> () {
}

#[with_sentinel_frame]
fn integer(
    a: usize,
    b: usize,
) -> usize {
    a + b
}

#[with_sentinel_frame]
fn never() -> ! {
    loop {}
}
