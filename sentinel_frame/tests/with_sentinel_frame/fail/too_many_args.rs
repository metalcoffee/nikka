use sentinel_frame::with_sentinel_frame;

#[with_sentinel_frame]
fn lots_of_args(
    _v1: usize,
    _v2: isize,
    _v3: u64,
    _v4: u32,
    _v5: u8,
    _v6: i8,
    _v7: i16,
) {
}

fn main() {
}
