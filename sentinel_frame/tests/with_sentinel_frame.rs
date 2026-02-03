#![deny(warnings)]

use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use sentinel_frame::with_sentinel_frame;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn read_counter() -> u64 {
    COUNTER.load(Ordering::Relaxed)
}

fn add_counter(v: u64) -> u64 {
    COUNTER.fetch_add(v, Ordering::Relaxed)
}

fn reset_counter() {
    COUNTER.store(0, Ordering::Relaxed);
}

#[with_sentinel_frame]
fn just_increment() {
    add_counter(1);
}

#[test]
fn function_is_invoked() {
    reset_counter();

    for i in 0 .. 5 {
        just_increment();
        assert_eq!(read_counter(), i + 1);
    }
}

#[derive(PartialEq, Eq, Debug)]
struct Values {
    a: u32,
    b: [u32; 3],
}

const TARGET: Values = Values {
    a: 43,
    b: [44, 45, 46],
};

#[with_sentinel_frame]
fn non_trivial_args(
    v1: usize,
    v2: i32,
    v3: &mut u32,
    v4: *const u32,
    v5: &Values,
    v6: &mut Values,
) -> u32 {
    *v3 = 42;
    *v6 = TARGET;

    v1 as u32 + v2 as u32 + unsafe { *v4 } + v5.a + v5.b.into_iter().sum::<u32>()
}

#[test]
fn arguments_are_passed_properly() {
    let mut v3 = 0;
    let v4 = 4u32;

    let v5 = Values {
        a: 8,
        b: [16, 32, 64],
    };
    let mut v6 = Values { a: 0, b: [0; 3] };

    let output = non_trivial_args(1, 2, &mut v3, &v4 as *const u32, &v5, &mut v6);
    assert_eq!(output, 127);

    assert_eq!(v3, 42);
    assert_eq!(v6, TARGET);
}

#[with_sentinel_frame]
extern "C" fn add(
    a: i32,
    b: i32,
) -> i32 {
    a + b
}

#[test]
fn c_abi() {
    for i in 0 .. 5 {
        for j in 0 .. 5 {
            assert_eq!(add(i, j), i + j);
        }
    }
}

#[test]
fn with_sentinel_frame_codegen() {
    const BASE_DIR: &str = "tests/with_sentinel_frame";

    // Compilation/compiler errors
    let t = trybuild::TestCases::new();
    t.compile_fail(format!("{BASE_DIR}/fail/*.rs"));
    t.pass(format!("{BASE_DIR}/pass/*.rs"));

    // Expansion
    // Requires cargo-expand
    macrotest::expand(format!("{BASE_DIR}/expand/*.rs"));
}
