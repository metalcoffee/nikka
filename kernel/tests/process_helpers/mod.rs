#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

use xmas_elf::ElfFile;

use ku::sync::spinlock::SpinlockGuard;

use kernel::{
    log::{
        debug,
        info,
    },
    memory::{
        FRAME_ALLOCATOR,
        Virt,
        test_scaffolding::translate,
    },
    process::{
        self,
        Pid,
        Process,
        Table,
        test_scaffolding,
    },
    trap::{
        TRAP_STATS,
        Trap,
    },
};

pub(super) fn make(file: &[u8]) -> Process {
    let start_free_frames = FRAME_ALLOCATOR.lock().count();

    let mut process =
        test_scaffolding::create_process(file).expect("failed to create the test process");

    check(file, &mut process);

    let process_frames = start_free_frames - FRAME_ALLOCATOR.lock().count();
    debug!(process_frames);
    assert!(process_frames > 0, "created process uses no memory");

    process
}

pub(super) fn dummy_allocate(file: &[u8]) -> SpinlockGuard<'static, Process> {
    test_scaffolding::init();

    let pid = test_scaffolding::allocate(
        test_scaffolding::create_process(file).expect("failed to create the test process"),
    )
    .unwrap();

    let mut process = Table::get(pid).expect("failed to find the new process in the process table");

    check(file, &mut process);

    process
}

pub(super) fn allocate(file: &[u8]) -> SpinlockGuard<'static, Process> {
    let start_free_frames = FRAME_ALLOCATOR.lock().count();

    let pid = process::create(file).expect("failed to create the test process");
    let mut process = Table::get(pid).expect("failed to find the new process in the process table");

    check(file, &mut process);

    let process_frames = start_free_frames - FRAME_ALLOCATOR.lock().count();
    debug!(process_frames);
    assert!(process_frames > 0, "created process uses no memory");

    process
}

pub(super) fn free(pid: Pid) {
    Table::free(pid).expect("failed to find the new process in the process table");
}

fn check(
    file: &[u8],
    process: &mut Process,
) {
    let entry_point = Virt::new_u64(ElfFile::new(file).unwrap().header.pt2.entry_point()).unwrap();

    let mapping_error =
        "the ELF file has not been loaded into the address space at the correct address";
    let pte = translate(process.address_space(), entry_point).expect(mapping_error);

    info!(
        %entry_point,
        frame = ?pte.frame().expect(mapping_error),
        flags = ?pte.flags(),
        "user process page table entry",
    );

    assert!(pte.is_present(), "{}", mapping_error);
    assert!(
        pte.is_user(),
        "the ELF file is not accessible from the user space",
    );
    assert!(pte.is_executable(), "the entry point is not executable");
}

#[must_use]
pub(super) fn forbid_traps() -> impl Drop {
    forbid_traps_except(&[])
}

#[must_use]
pub(super) fn forbid_traps_except(allowed_traps: &[Trap]) -> impl Drop {
    return scopeguard::guard(trap_counts(), |start_trap_counts| {
        let end_trap_counts = trap_counts();
        for ((trap, start_count), (same_trap, end_count)) in
            start_trap_counts.into_iter().zip(end_trap_counts)
        {
            if allowed_traps.iter().all(|&allowed_trap| allowed_trap != trap) {
                assert_eq!(trap, same_trap);
                assert!(start_count <= end_count);

                assert_eq!(
                    start_count,
                    end_count,
                    "unexpected trap {:?} fired {} times",
                    trap,
                    end_count - start_count,
                );
            }
        }
    });

    fn trap_counts() -> Vec<(Trap, usize)> {
        enum_iterator::all::<Trap>()
            .filter(|&trap| trap <= Trap::SecurityException)
            .map(|trap| (trap, TRAP_STATS[trap].count()))
            .collect()
    }
}

#[macro_export]
macro_rules! page_aligned {
    ($path:expr) => {{
        #[repr(C, align(4096))]
        struct PageAligned<T: ?Sized>(T);

        const BYTES: &'static PageAligned<[u8]> = &PageAligned(*include_bytes!($path));

        &BYTES.0
    }};
}
