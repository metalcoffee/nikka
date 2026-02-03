#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![feature(iterator_try_reduce)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use chrono::Duration;

use ku::{
    error::Error::{
        InvalidAlignment,
        InvalidArgument,
        NoPage,
        Overflow,
        PermissionDenied,
    },
    memory::{
        Block,
        Page,
        mmu::{
            KERNEL_R,
            KERNEL_RW,
            USER_R,
            USER_RW,
        },
    },
    process::Pid,
    sync::spinlock::Spinlock,
};

use kernel::{
    Subsystems,
    log::debug,
    memory::test_scaffolding::switch_to,
    process::{
        Process,
        Scheduler,
        test_scaffolding::{
            copy_mapping,
            map,
            set_pid,
            unmap,
        },
    },
    time::{
        self,
        TscDuration,
    },
    trap::{
        TRAP_STATS,
        Trap,
    },
};

mod init;
mod mm_helpers;
mod process_helpers;

init!(Subsystems::MEMORY | Subsystems::SYSCALL | Subsystems::SMP | Subsystems::PROCESS);

const MEMORY_ALLOCATOR_ELF: &[u8] = page_aligned!("../../target/kernel/user/memory_syscalls");

#[test_case]
fn map_syscall_group() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let process = Spinlock::new(process_helpers::make(MEMORY_ALLOCATOR_ELF));
    set_pid(&mut process.lock(), Pid::new(0));
    let pid = process.lock().pid().into_usize();
    switch_to(process.lock().address_space());

    let kernel_block = Block::from_slice("some kernel memory".as_bytes()).enclosing();
    map_over_kernel(&process, kernel_block);

    let kernel_block = process
        .lock()
        .address_space()
        .allocate(Page::layout_array(2), KERNEL_RW)
        .unwrap();
    let mut head = kernel_block;
    let tail = head.tail(1).unwrap();
    unsafe {
        process.lock().address_space().map_block(tail, KERNEL_RW).unwrap();
    }
    map_over_kernel(&process, kernel_block);
    unsafe {
        process.lock().address_space().unmap_block(tail).unwrap();
    }

    let size = kernel_block.size();
    let layout = Page::layout(size).unwrap();
    let kernel_block = process.lock().address_space().allocate(layout, KERNEL_RW).unwrap();
    let kernel_address = kernel_block.start_address().into_usize();
    let new_kernel_block = process.lock().address_space().allocate(layout, KERNEL_RW).unwrap();
    let new_kernel_address = new_kernel_block.start_address().into_usize();
    let user_block = process.lock().address_space().allocate(layout, USER_RW).unwrap();
    let user_address = user_block.start_address().into_usize();
    let new_user_block = process.lock().address_space().allocate(layout, USER_RW).unwrap();
    let new_user_address = new_user_block.start_address().into_usize();

    for flags in [KERNEL_R, KERNEL_RW, USER_R, USER_RW] {
        let flags = flags.bits();
        assert_eq!(
            map(process.lock(), pid, kernel_address, size, flags),
            Err(PermissionDenied),
        );

        for (src_address, dst_address) in [
            (kernel_address, new_kernel_address),
            (kernel_address, new_user_address),
            (user_address, new_kernel_address),
        ] {
            assert_eq!(
                copy_mapping(process.lock(), pid, src_address, dst_address, size, flags),
                Err(PermissionDenied),
            );
        }
    }

    for flags in [USER_R, USER_RW] {
        let flags = flags.bits();
        assert_eq!(
            map(process.lock(), pid, user_address, size, flags),
            Ok(user_address),
        );

        let allocated_memory = unsafe { user_block.try_into_slice::<usize>().unwrap() };
        assert!(
            allocated_memory.iter().all(|&x| x == 0),
            "do not leak information into the user space in allocated frames",
        );

        for flags in [KERNEL_R, KERNEL_RW] {
            let flags = flags.bits();
            assert_eq!(
                copy_mapping(
                    process.lock(),
                    pid,
                    user_address,
                    new_user_address,
                    size,
                    flags,
                ),
                Err(PermissionDenied),
            );
        }

        assert!(
            copy_mapping(
                process.lock(),
                pid,
                user_address,
                new_user_address,
                size,
                flags,
            )
            .is_ok()
        );
    }

    let flags = USER_RW.bits();
    assert!(unmap(process.lock(), pid, user_address, size).is_ok());
    assert!(unmap(process.lock(), pid, new_user_address, size).is_ok());
    assert_eq!(
        copy_mapping(
            process.lock(),
            pid,
            user_address,
            new_user_address,
            size,
            flags,
        ),
        Err(NoPage),
    );
    assert!(unmap(process.lock(), pid, user_address, size).is_err());

    assert!(map(process.lock(), pid, 0, size, flags).is_ok());
    assert_eq!(
        map(process.lock(), pid, 1, size, flags),
        Err(InvalidAlignment),
    );
    for address in [0, user_address] {
        assert_eq!(
            map(process.lock(), pid, address, 0, flags),
            Err(InvalidArgument),
        );
        for size in [1, size + 1] {
            assert_eq!(
                map(process.lock(), pid, address, size, flags),
                Err(InvalidAlignment),
            );
        }
    }

    for (address, size) in [
        (0x1_0000, 0xFFFF_FFFF_0000_0000),
        (0xFFFF_FFFF_FFFF_0000, 0x10_0000),
    ] {
        let result = map(process.lock(), pid, address, size, flags);
        assert!(
            result == Err(InvalidArgument) || result == Err(Overflow),
            "expected Err(InvalidArgument) or Err(Overflow), got {result:?}",
        );
    }
}

fn map_over_kernel(
    process: &Spinlock<Process>,
    block: Block<Page>,
) {
    let pid = process.lock().pid().into_usize();

    let address = block.start_address().into_usize();
    let size = block.size();

    assert_eq!(
        unmap(process.lock(), pid, address, size),
        Err(PermissionDenied),
    );

    for flags in [KERNEL_R, KERNEL_RW, USER_R, USER_RW] {
        let new_block = process.lock().address_space().allocate(block.layout(), flags).unwrap();
        let new_address = new_block.start_address().into_usize();
        debug!(%flags, %new_block, %new_address);
        let flags = flags.bits();

        assert_eq!(
            map(process.lock(), pid, address, size, flags),
            Err(PermissionDenied),
        );
        assert_eq!(
            copy_mapping(process.lock(), pid, address, new_address, size, flags),
            Err(PermissionDenied),
        );
    }
}

#[test_case]
fn copy_mapping_of_intersecting_blocks() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let process = Spinlock::new(process_helpers::make(MEMORY_ALLOCATOR_ELF));
    set_pid(&mut process.lock(), Pid::new(0));
    let pid = process.lock().pid().into_usize();
    switch_to(process.lock().address_space());

    let block = process.lock().address_space().allocate(Page::layout_array(5), USER_RW).unwrap();
    let old = block.slice(1 .. block.count() - 1).unwrap();
    let old_address = old.start_address().into_usize();
    let size = old.size();
    let flags = USER_R.bits();

    let mut transactional = true;
    let mut intersection_is_supported = true;

    let try_unmap_page = |page: Page| -> bool {
        match unmap(process.lock(), pid, page.address().into_usize(), Page::SIZE) {
            Ok(_) => true,
            Err(error) => {
                assert_eq!(error, NoPage);
                false
            },
        }
    };

    for hole in [None, old.into_iter().nth(1)] {
        for new_offset in 0 ..= 2 {
            let new = block.slice(new_offset .. block.count() - (2 - new_offset)).unwrap();
            let new_address = new.start_address().into_usize();

            for page in old {
                assert!(!try_unmap_page(page));
            }

            assert_eq!(
                map(process.lock(), pid, old_address, size, flags,),
                Ok(old_address),
            );

            if let Some(hole) = hole {
                assert!(try_unmap_page(hole));
            }

            for page in new {
                if !old.contains(page) {
                    assert!(!try_unmap_page(page));
                }
            }

            let result = copy_mapping(process.lock(), pid, old_address, new_address, size, flags);

            debug!(%old, %new, ?hole, ?result, "copy mapping of intersecting blocks");

            if hole.is_some() {
                assert!(result.is_err());

                for page in new {
                    if !old.contains(page) && try_unmap_page(page) {
                        debug!(
                            %page,
                            "failed copy_mapping() syscall \
                             has modified the page mapping of the new block",
                        );
                        transactional = false;
                    }
                }
            } else if result.is_ok() {
                for page in new {
                    if !old.contains(page) {
                        assert!(try_unmap_page(page));
                    }
                }
            } else {
                intersection_is_supported = false;
                assert_eq!(result, Err(InvalidArgument));
            }

            for page in old {
                assert_eq!(try_unmap_page(page), hole != Some(page));
            }
        }
    }

    debug!(
        intersection_is_supported,
        transactional,
        "{}",
        if intersection_is_supported && transactional {
            "congratulations, copy_mapping() is transactional and supports intersecting blocks"
        } else {
            "copy_mapping()"
        }
    );
}

#[test_case]
fn copy_mapping_of_enormous_blocks() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let process = Spinlock::new(process_helpers::make(MEMORY_ALLOCATOR_ELF));
    set_pid(&mut process.lock(), Pid::new(0));
    let pid = process.lock().pid().into_usize();
    switch_to(process.lock().address_space());

    let mut old = process
        .lock()
        .address_space()
        .allocate(Page::layout_array(1 << 20), USER_RW)
        .unwrap();
    let new = old.tail(old.count() / 2).unwrap();

    let old_address = old.start_address().into_usize();
    let new_address = new.start_address().into_usize();
    let size = old.size();
    let flags = USER_R.bits();

    let iterations = 1_000;
    let timeout = TscDuration::try_from(Duration::seconds(2)).unwrap();
    let timer = time::timer();

    for iteration in 1 ..= iterations {
        assert_eq!(
            copy_mapping(process.lock(), pid, old_address, new_address, size, flags),
            Err(NoPage),
        );

        let elapsed = timer.elapsed();

        assert!(
            elapsed < timeout,
            "only {iteration} iterations of copy_mapping() in {elapsed}",
        );
    }

    let elapsed = timer.elapsed();
    debug!(iterations, %elapsed, "iterations of copy_mapping()");
}

#[test_case]
fn user_space_memory_allocator() {
    let _trap_guard = process_helpers::forbid_traps();
    let _guard = mm_helpers::forbid_frame_leaks();

    Scheduler::enqueue(process_helpers::allocate(MEMORY_ALLOCATOR_ELF).pid());

    while Scheduler::run_one() {}

    assert_eq!(
        TRAP_STATS[Trap::PageFault].count(),
        0,
        "the user mode code has detected an error in the memory allocator implementation",
    );
}
