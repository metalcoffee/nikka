#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::memory::size::MiB;

use kernel::{
    Subsystems,
    log::debug,
    memory::{
        FRAME_ALLOCATOR,
        Frame,
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::MEMORY);

#[test_case]
fn sanity_check() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let frame_allocator = FRAME_ALLOCATOR.lock();
    let free_frames = frame_allocator.count();

    let qemu_memory_frames = 128 * MiB / Frame::SIZE;
    let min_free_frames = qemu_memory_frames - 16 * MiB / Frame::SIZE;

    debug!(free_frames, min_free_frames, qemu_memory_frames);

    assert!(free_frames > min_free_frames);
    assert!(free_frames < qemu_memory_frames);
}
