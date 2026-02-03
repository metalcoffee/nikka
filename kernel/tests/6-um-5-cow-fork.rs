#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::{
    format,
    string::String,
    vec::Vec,
};
use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use ku::{
    process::Pid,
    sync::Spinlock,
};

use kernel::{
    Subsystems,
    log::debug,
    memory::FRAME_ALLOCATOR,
    process::{
        Process,
        Scheduler,
        test_scaffolding::set_pid_callback,
    },
    trap::{
        TRAP_STATS,
        Trap,
    },
};

mod init;
mod mm_helpers;
mod process_helpers;

init!(Subsystems::MEMORY | Subsystems::SMP | Subsystems::PROCESS);

const COW_FORK_ELF: &[u8] = page_aligned!("../../target/kernel/user/cow_fork");

#[test_case]
fn cow_fork() {
    let _trap_guard = process_helpers::forbid_traps_except(&[Trap::PageFault]);

    static START_FREE_FRAMES: AtomicUsize = AtomicUsize::new(0);
    static ONE_PROCESS_FRAMES: AtomicUsize = AtomicUsize::new(0);
    static PARENTS: Spinlock<Vec<(Pid, Pid)>> = Spinlock::new(Vec::new());

    *PARENTS.lock() = Vec::with_capacity(16);

    set_pid_callback(record_parent);
    fn record_parent(process: &Process) {
        if let Some(parent) = process.parent() {
            let mut parents = PARENTS.lock();
            parents.push((parent, process.pid()));

            let one_process_frames = ONE_PROCESS_FRAMES.load(Ordering::Relaxed);
            let start_free_frames = START_FREE_FRAMES.load(Ordering::Relaxed);
            let used_frames = start_free_frames - FRAME_ALLOCATOR.lock().count();

            debug!(
                "physical frames: {}% shared, {} per process, {} total, {} processes",
                100 - 100 * used_frames / ((1 + parents.len()) * one_process_frames),
                one_process_frames,
                used_frames,
                1 + parents.len(),
            );
            assert!(
                used_frames < 70 * (1 + parents.len()) * one_process_frames / 100,
                "less than 30% of the physical frames are shared: {} per process, {} total, {} \
                 processes",
                one_process_frames,
                used_frames,
                1 + parents.len(),
            );
        }
    }

    {
        let _guard = mm_helpers::forbid_frame_leaks();

        START_FREE_FRAMES.store(FRAME_ALLOCATOR.lock().count(), Ordering::Relaxed);
        let pid = process_helpers::allocate(COW_FORK_ELF).pid();
        ONE_PROCESS_FRAMES.store(
            START_FREE_FRAMES.load(Ordering::Relaxed) - FRAME_ALLOCATOR.lock().count(),
            Ordering::Relaxed,
        );

        Scheduler::enqueue(pid);

        while Scheduler::run_one() {}
    }

    assert!(
        TRAP_STATS[Trap::PageFault].count() > 100,
        "cow_fork should page fault a lot",
    );

    let mut parents = PARENTS.lock();

    assert_eq!(parents.len(), 12, "wrong total number of child processes");

    introsort::sort(&mut parents);

    for children in parents.chunk_by(|a, b| a.0 == b.0) {
        assert_eq!(
            children.len(),
            3,
            "wrong number of children {} for the process {}",
            children.len(),
            children[0].0,
        );
    }

    let mut graphviz =
        String::from("digraph process_tree { node [ style = filled; fillcolor = \"#CCCCCC\"]; ");
    for (parent, process) in parents.iter() {
        debug!(%parent, %process);
        graphviz += &format!("\"{parent}\" -> \"{process}\"; ")
    }
    graphviz += "}";
    debug!(%graphviz);

    *parents = Vec::new();
}
