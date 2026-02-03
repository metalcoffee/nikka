use sentinel_frame::with_sentinel_frame;

struct Test(i32);

#[with_sentinel_frame]
fn f1(
    _a: (),
    _b: (usize,),
    _c: Test,
) {
}

#[with_sentinel_frame]
fn f2(Test(_v): Test) {
}

fn main() {
}
