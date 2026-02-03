#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use kernel::{
    Subsystems,
    log::info,
    trap::{
        TRAP_STATS,
        Trap,
    },
};

mod init;

init!(Subsystems::empty());

fn emit_breakpoint_trap() {
    info!("emitting breakpoint interrupt (int3)");
    x86_64::instructions::interrupts::int3();
}

#[test_case]
fn traps_are_working() {
    emit_breakpoint_trap();
}

#[test_case]
fn trap_counter() {
    let breakpoint_counter = &TRAP_STATS[Trap::Breakpoint];
    let begin_count = breakpoint_counter.count();
    let count = 3;
    for _ in 0 .. count {
        emit_breakpoint_trap();
    }
    let end_count = breakpoint_counter.count();
    info!(begin_count, count, end_count);
    assert_eq!(begin_count + count, end_count);
}
