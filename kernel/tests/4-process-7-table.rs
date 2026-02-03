#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::vec::Vec;

use kernel::{
    Subsystems,
    log::debug,
    process::test_scaffolding::{
        PROCESS_SLOT_COUNT,
        dummy_process,
    },
};

mod init;
mod mm_helpers;
mod process_helpers;

init!(Subsystems::MEMORY | Subsystems::PROCESS);

#[test_case]
fn basic() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let pid_1 = dummy_process().unwrap();
    let pid_2 = dummy_process().unwrap();

    debug!(%pid_1, %pid_2);
    assert_ne!(pid_1.slot(), pid_2.slot());

    process_helpers::free(pid_1);
    process_helpers::free(pid_2);
}

// Temporarily disabled for being too slow.
#[allow(unused)]
fn full_capacity() {
    let mut pids = Vec::with_capacity(PROCESS_SLOT_COUNT);
    let mut slots = Vec::with_capacity(PROCESS_SLOT_COUNT);

    let frame_leak_guard = mm_helpers::forbid_frame_leaks();

    while pids.len() < PROCESS_SLOT_COUNT {
        let pid = dummy_process().expect("failed to create the test process");
        pids.push(pid);
        slots.push(pid.slot());
    }

    dummy_process().expect_err("the process table is bigger than it should be");

    introsort::sort(slots.as_mut_slice());
    let slots_have_expected_range = slots.iter().enumerate().all(|(index, slot)| index == *slot);
    assert!(slots_have_expected_range);

    let mut prev_pid = pids[0];
    for _ in 0 .. 10 {
        process_helpers::free(pids[0]);
        let pid = dummy_process().expect("failed to create the test process");
        pids[0] = pid;

        dummy_process().expect_err("the process table is bigger than it should be");

        debug!(%prev_pid, %pid);
        assert_ne!(prev_pid, pid);

        prev_pid = pid;
    }

    while let Some(pid) = pids.pop() {
        process_helpers::free(pid);
    }

    // Ensure that the vectors outlive the frame leak guard so it will not account
    // for both their allocate and deallocate.
    drop(frame_leak_guard);
    drop(slots);
    drop(pids);
}
