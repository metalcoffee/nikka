#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use kernel::{
    Subsystems,
    log::info,
    process,
};

mod init;
mod mm_helpers;
mod process_helpers;

init!(Subsystems::MEMORY);

const LOOP_ELF: &[u8] = page_aligned!("../../target/kernel/user/loop");

#[test_case]
fn create_process() {
    let _guard = mm_helpers::forbid_frame_leaks();

    process_helpers::make(LOOP_ELF);
}

#[test_case]
fn create_process_failure() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let bad_elf_file: &[u8] = &[];
    let error = process::create(bad_elf_file).expect_err("created a process from a bad ELF file");

    info!(?error, "expected a process creation failure");
}
