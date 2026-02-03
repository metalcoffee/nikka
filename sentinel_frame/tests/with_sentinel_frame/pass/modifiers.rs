use sentinel_frame::with_sentinel_frame;

#[allow(unused_mut)]
#[with_sentinel_frame]
pub fn run<'a>(
    mut _v1: &'a mut i32,
    mut _v2: &'a mut i32,
) -> () {
}

fn main() {
}
