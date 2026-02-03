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

use ku::{
    error::Error::{
        InvalidArgument,
        PermissionDenied,
    },
    process::{
        Pid,
        State,
    },
    sync::Spinlock,
};

use kernel::{
    Subsystems,
    log::debug,
    memory::test_scaffolding::switch_to,
    process::{
        Process,
        Scheduler,
        Table,
        test_scaffolding::{
            exofork,
            set_pid_callback,
            set_state,
        },
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

const EAGER_FORK_ELF: &[u8] = page_aligned!("../../target/kernel/user/eager_fork");
const EXIT_ELF: &[u8] = page_aligned!("../../target/kernel/user/exit");

#[test_case]
fn t1_exofork_syscall() {
    let _trap_guard = process_helpers::forbid_traps_except(&[Trap::PageFault]);
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut parent = process_helpers::allocate(EXIT_ELF);
    let parent_pid = parent.pid();
    let unrelated_process_pid = process_helpers::allocate(EXIT_ELF).pid();

    switch_to(parent.address_space());

    let child_pid = Pid::from_usize(exofork(parent).expect("exofork() failed"))
        .expect("wrong child pid from exofork()");

    debug!(%child_pid);

    let child =
        Table::get(child_pid).expect("failed to find the child process in the process table");

    debug!(%child);

    drop(child);

    assert_eq!(
        set_state(
            Table::get(parent_pid).unwrap(),
            unrelated_process_pid.into_usize(),
            State::Runnable.into(),
        ),
        Err(PermissionDenied),
    );

    assert_eq!(
        set_state(Table::get(parent_pid).unwrap(), child_pid.into_usize(), 42),
        Err(InvalidArgument),
    );

    assert_eq!(
        set_state(
            Table::get(parent_pid).unwrap(),
            child_pid.into_usize(),
            State::Running.into(),
        ),
        Err(InvalidArgument),
    );

    let result = set_state(
        Table::get(parent_pid).unwrap(),
        child_pid.into_usize(),
        State::Runnable.into(),
    );
    assert!(result.is_ok());

    assert_eq!(
        set_state(
            Table::get(parent_pid).unwrap(),
            child_pid.into_usize(),
            State::Runnable.into(),
        ),
        Err(PermissionDenied),
    );

    for pid in [parent_pid, unrelated_process_pid] {
        process_helpers::free(pid);
    }

    let start_page_faults = TRAP_STATS[Trap::PageFault].count();

    while Scheduler::run_one() {}

    assert_eq!(
        TRAP_STATS[Trap::PageFault].count(),
        start_page_faults + 1,
        "the child process should page fault",
    );

    Table::get(child_pid).expect_err("the child process has not run up to its completion");
}

#[test_case]
fn t2_eager_fork() {
    let _trap_guard = process_helpers::forbid_traps();

    let start_page_fault_count = TRAP_STATS[Trap::PageFault].count();

    static PARENTS: Spinlock<Vec<(Pid, Pid)>> = Spinlock::new(Vec::new());

    *PARENTS.lock() = Vec::with_capacity(16);

    set_pid_callback(record_parent);
    fn record_parent(process: &Process) {
        if let Some(parent) = process.parent() {
            PARENTS.lock().push((parent, process.pid()));
        }
    }

    {
        let _guard = mm_helpers::forbid_frame_leaks();

        let pid = process_helpers::allocate(EAGER_FORK_ELF).pid();

        Scheduler::enqueue(pid);

        while Scheduler::run_one() {}
    }

    assert_eq!(
        TRAP_STATS[Trap::PageFault].count(),
        start_page_fault_count,
        "eager_fork should not page fault",
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
