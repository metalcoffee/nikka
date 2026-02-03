#![deny(warnings)]
#![feature(allocator_api)]
#![feature(custom_test_frameworks)]
#![feature(ptr_as_uninit)]
#![feature(slice_ptr_get)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

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
};

use ku::{
    allocator::Info,
    memory::{
        Page,
        Virt,
        size::{
            MiB,
            Size,
        },
    },
};

use kernel::{
    Subsystems,
    allocator,
    log::debug,
};

mod init;

init!(Subsystems::MEMORY);

#[test_case]
fn basic() {
    memory_allocator_basic();
}

#[test_case]
fn alignment() {
    memory_allocator_alignment();
}

#[test_case]
fn grow_and_shrink() {
    memory_allocator_grow_and_shrink();
}

#[test_case]
fn stress() {
    let values = 100_000;
    let max_fragmentation_loss = |values| cmp::max(6 * values, 2 * MiB);
    let pages_for_values = memory_allocator_stress(values, max_fragmentation_loss);
    debug!(values, pages_for_values);
    assert!(pages_for_values < 2 * values / mem::size_of::<usize>());
}

macro_rules! my_assert {
    ($cond:expr $(,)?) => {{
        assert!($cond);
    }};
}

include!("include/memory_allocator.rs");
