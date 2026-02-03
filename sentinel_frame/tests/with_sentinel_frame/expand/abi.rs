use sentinel_frame::with_sentinel_frame;

#[with_sentinel_frame]
extern "C" fn add(
    a: i32,
    b: i32,
) -> i32 {
    a + b
}

#[with_sentinel_frame]
extern "C" fn never() -> ! {
    loop {}
}
