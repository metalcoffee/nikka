#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use kernel::{
    Subsystems,
    log::debug,
    process::{
        Process,
        Scheduler,
        Table,
        test_scaffolding,
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

const EXIT_ELF: &[u8] = page_aligned!("../../target/kernel/user/exit");
const PAGE_FAULT_ELF: &[u8] = page_aligned!("../../target/kernel/user/page_fault");
const SCHED_YIELD_ELF: &[u8] = page_aligned!("../../target/kernel/user/sched_yield");

#[test_case]
fn syscall_sched_yield() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut process = process_helpers::allocate(SCHED_YIELD_ELF);
    let pid = process.pid();

    test_scaffolding::disable_interrupts(&mut process);

    let start_page_faults = TRAP_STATS[Trap::PageFault].count();

    for _ in 0 .. 3 {
        Process::enter_user_mode(process);

        process = Table::get(pid).expect("failed to find the process in the process table");

        let user_registers = test_scaffolding::registers(&process);

        debug!(?user_registers, "returned from the user space");

        assert!(
            test_scaffolding::scheduler_has_pid(pid),
            "the process was not enqueued back to the scheduler",
        );
        assert_eq!(
            TRAP_STATS[Trap::PageFault].count(),
            start_page_faults,
            "maybe the read-dont-modify-write construction was used for reading the RTC time from \
             the user space",
        );
    }

    drop(process);

    process_helpers::free(pid);
}

#[test_case]
fn scheduler() {
    let exit_pid = process_helpers::allocate(EXIT_ELF).pid();
    let page_fault_pid = process_helpers::allocate(PAGE_FAULT_ELF).pid();

    Scheduler::enqueue(exit_pid);
    Scheduler::enqueue(page_fault_pid);

    while Scheduler::run_one() {}

    Table::get(exit_pid).expect_err("the 'exit' process was not run up to its completion");
    Table::get(page_fault_pid)
        .expect_err("the 'page_fault' process was not run up to its completion");
}

#[test_case]
fn sched_yield_reschedules() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut process = process_helpers::allocate(SCHED_YIELD_ELF);
    let pid = process.pid();

    test_scaffolding::disable_interrupts(&mut process);
    drop(process);
    Scheduler::enqueue(pid);

    for _ in 0 .. 3 {
        assert!(
            Scheduler::run_one(),
            "The process hasn't rescheduled itself",
        );
    }

    process_helpers::free(pid);
    while Scheduler::run_one() {}
}
