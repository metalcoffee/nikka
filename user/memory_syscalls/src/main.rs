#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![no_main]
#![no_std]

extern crate alloc;

use alloc::{
    alloc::{
        Allocator,
        Global,
        Layout,
    },
    boxed::Box,
    collections::BTreeMap,
    vec::Vec,
};
use core::{
    cmp,
    mem,
    panic::PanicInfo,
    ptr::NonNull,
};

use ku::{
    allocator::Info,
    log::{
        debug,
        error,
        info,
    },
    memory::{
        Page,
        Virt,
        size::{
            KiB,
            MiB,
            Size,
        },
    },
};

use lib::{
    allocator,
    entry,
};

entry!(main);

fn main() {
    lib::set_panic_handler(panic_handler);

    info!(test_case = "basic");
    memory_allocator_basic();

    info!(test_case = "alignment");
    memory_allocator_alignment();

    info!(test_case = "grow_and_shrink");
    memory_allocator_grow_and_shrink();

    info!(test_case = "stress");
    let values = 10_000;
    let max_fragmentation_loss = |values| cmp::max(8 * KiB * values, 16 * MiB);
    memory_allocator_stress(values, max_fragmentation_loss);
}

fn generate_page_fault() -> ! {
    unsafe {
        NonNull::<u8>::dangling().as_ptr().read_volatile();
    }

    unreachable!();
}

fn panic_handler(_: &PanicInfo) {
    generate_page_fault();
}

macro_rules! my_assert {
    ($condition:expr $(,)?) => {{
        if !$condition {
            error!(condition = stringify!($condition), "assert failed");
            generate_page_fault();
        }
    }};
}

include!("../../../kernel/tests/include/memory_allocator.rs");
