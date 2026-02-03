use sentinel_frame::with_sentinel_frame;

#[with_sentinel_frame]
fn simple() -> i32 {
    42
}

#[with_sentinel_frame]
fn with_args(
    a: i32,
    b: &i32,
    c: &mut i32,
) {
    *c = a + *b;
}
