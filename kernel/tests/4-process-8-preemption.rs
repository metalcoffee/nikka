#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::process::State;

use kernel::{
    Subsystems,
    log::debug,
    process::{
        Process,
        Table,
        test_scaffolding,
    },
};

mod init;
mod mm_helpers;
mod process_helpers;

init!(Subsystems::MEMORY | Subsystems::SMP | Subsystems::PROCESS);

const CHECK_CONTEXT_ELF: &[u8] = page_aligned!("../../target/kernel/user/check_context");
const LOOP_ELF: &[u8] = page_aligned!("../../target/kernel/user/loop");

#[test_case]
fn preemption() {
    let _trap_guard = process_helpers::forbid_traps();
    let _guard = mm_helpers::forbid_frame_leaks();

    let pid = process_helpers::allocate(LOOP_ELF).pid();

    let process = Table::get(pid).expect("failed to find the new process in the process table");
    Process::enter_user_mode(process);

    // If this does not happen and the test times out, the preemption is not working properly.
    debug!("returned from the user space");

    process_helpers::free(pid);
}

#[test_case]
fn flags_and_registers_are_saved() {
    let _trap_guard = process_helpers::forbid_traps();
    let _guard = mm_helpers::forbid_frame_leaks();

    let message = "check that all registers including `rflags` are saved correctly when the \
                   process is preempted";
    let pid = process_helpers::allocate(CHECK_CONTEXT_ELF).pid();

    for _ in 0 .. 50 {
        let process = Table::get(pid).expect(message);
        assert_eq!(
            test_scaffolding::state(&process),
            State::Runnable,
            "{message}",
        );
        assert!(Process::enter_user_mode(process), "{message}");
    }

    process_helpers::free(pid);
}
