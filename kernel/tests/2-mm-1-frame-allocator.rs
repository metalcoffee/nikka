#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use core::mem;

use ku::{
    memory::size::{
        GiB,
        MiB,
    },
    sync::Spinlock,
};

use kernel::{
    Subsystems,
    error::Error::NoFrame,
    log::debug,
    memory::{
        BASE_ADDRESS_SPACE,
        Block,
        FRAME_ALLOCATOR,
        Frame,
        FrameGuard,
        Page,
        Phys,
        test_scaffolding::phys2virt,
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::PHYS_MEMORY);

#[test_case]
fn t1_sanity_check() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let frame_allocator = FRAME_ALLOCATOR.lock();
    let free_frames = frame_allocator.count();

    let qemu_memory_frames = 128 * MiB / Frame::SIZE;
    let min_free_frames = 16 * MiB / Frame::SIZE;

    debug!(free_frames, min_free_frames, qemu_memory_frames);

    assert!(free_frames > min_free_frames);
    assert!(free_frames < qemu_memory_frames);
}

#[test_case]
fn t2_basic_frame_allocator_functions() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut frame_allocator = FRAME_ALLOCATOR.lock();
    let start_free_frames = frame_allocator.count();
    let frame_count = init::frame_count();

    let frames = [
        take(frame_allocator.allocate().unwrap()),
        take(frame_allocator.allocate().unwrap()),
    ];

    debug!(?frames, frame_count);
    assert_ne!(frames[0], frames[1]);
    assert_eq!(frame_allocator.count(), start_free_frames - 2);

    for frame in frames {
        assert!(
            frame.index() < frame_count,
            "allocated a frame outside of the physical memory of the current machine",
        );
    }

    frame_allocator.deallocate(frames[0]);
    assert_eq!(frame_allocator.count(), start_free_frames - 1);

    let reallocate_last_freed_frame = take(frame_allocator.allocate().unwrap());

    debug!(?reallocate_last_freed_frame);
    assert_eq!(reallocate_last_freed_frame, frames[0]);
    assert_eq!(frame_allocator.count(), start_free_frames - 2);

    take(frame_allocator.reference(frames[1]));
    assert_eq!(frame_allocator.count(), start_free_frames - 2);

    frame_allocator.deallocate(frames[1]);
    assert_eq!(frame_allocator.count(), start_free_frames - 2);

    frame_allocator.deallocate(frames[1]);
    assert_eq!(frame_allocator.count(), start_free_frames - 1);

    frame_allocator.deallocate(frames[0]);

    let unmanaged_frame = Frame::from_index(2 * init::frame_count()).unwrap();
    take(frame_allocator.reference(unmanaged_frame));
    frame_allocator.deallocate(unmanaged_frame);
}

#[test_case]
fn t3_allocated_frames_are_not_used() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let poison = 0xDEAD_BEEF_DEAD_BEEF_u64;

    let phys2virt = phys2virt(&BASE_ADDRESS_SPACE.lock());
    let mut frame_allocator = FRAME_ALLOCATOR.lock();
    let start_free_frames = frame_allocator.count();
    let frame_count = init::frame_count();
    debug!(start_free_frames, frame_count);

    static FRAMES: Spinlock<[Option<FrameGuard>; GiB / Frame::SIZE]> =
        Spinlock::new([const { None }; _]);
    let mut frames = FRAMES.lock();
    assert!(
        frames.len() >= frame_count,
        "reserve more memory for frames",
    );

    let mut frames_poisoned = 0;

    while let Ok(frame_guard) = frame_allocator.allocate() {
        let frame = *frame_guard;

        assert!(
            frame.index() < frame_count,
            "allocated a frame {frame} outside of the physical memory of the current machine with \
             only {frame_count} frames total",
        );

        frames[frames_poisoned] = Some(frame_guard);

        let page = Page::containing(phys2virt.map(frame.address()).unwrap());
        let page_block = Block::new(page, (page + 1).unwrap()).unwrap();

        let slice = unsafe { page_block.try_into_mut_slice().unwrap() };
        slice.fill(poison);

        if frames_poisoned % 1000 == 0 {
            debug!(
                frames_poisoned,
                %frame,
                poison = slice[frames_poisoned % slice.len()],
                "poisoning the free memory",
            );
        }

        frames_poisoned += 1;
    }

    let end_free_frames = frame_allocator.count();
    debug!(start_free_frames, end_free_frames, frames_poisoned);
    assert_eq!(end_free_frames, 0);
    assert_eq!(frames_poisoned, start_free_frames);
    assert_eq!(frame_allocator.allocate(), Err(NoFrame));

    introsort::sort(&mut frames[.. frames_poisoned]);
    for (a, b) in frames.iter().zip(&frames[1 .. frames_poisoned]) {
        assert_ne!(a, b, "allocated the same frame twice");
    }

    drop(frame_allocator);
    for frame in &mut *frames {
        *frame = None;
    }

    let free_frames = FRAME_ALLOCATOR.lock().count();
    debug!(free_frames);
    assert_eq!(free_frames, start_free_frames);
}

#[test_case]
fn t4_operations_on_absent_frames_are_ok() {
    for absent_phys in [0x0, 0xA_0000, 0xFEE0_0000, (1 << Phys::BITS) - 1] {
        let absent = Frame::containing(Phys::new(absent_phys).unwrap());
        let mut frame_allocator = FRAME_ALLOCATOR.lock();

        assert_eq!(frame_allocator.reference_count(absent), Err(NoFrame));
        assert!(frame_allocator.is_used(absent));

        let absent_guard = frame_allocator.reference(absent);
        assert_eq!(*absent_guard, absent);

        frame_allocator.deallocate(absent);

        drop(frame_allocator);
    }
}

fn take(frame_guard: FrameGuard) -> Frame {
    let frame = *frame_guard;
    mem::forget(frame_guard);

    frame
}
