#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(ptr_as_uninit)]
#![feature(slice_ptr_get)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::{
    error::Error::{
        InvalidArgument,
        NoPage,
        Overflow,
        PermissionDenied,
    },
    log::{
        self,
        Level,
    },
    memory::{
        Block,
        mmu::USER_RW,
        size,
    },
    process::Pid,
    sync::spinlock::Spinlock,
};

use kernel::{
    Subsystems,
    memory::test_scaffolding::{
        switch_to,
        user_pages,
    },
    process::test_scaffolding::{
        log_value,
        set_pid,
    },
};

mod init;
mod process_helpers;

init!(Subsystems::MEMORY | Subsystems::SYSCALL);

const LOOP_ELF: &[u8] = page_aligned!("../../target/kernel/user/loop");

#[test_case]
fn log_value_implementation() {
    let process = Spinlock::new(process_helpers::make(LOOP_ELF));
    set_pid(&mut process.lock(), Pid::new(0));
    switch_to(process.lock().address_space());

    let info = size::from(u32::from(log::level_into_symbol(&Level::INFO)));

    let block = Block::from_slice("some kernel memory".as_bytes());
    assert_eq!(
        log_value(
            process.lock(),
            info,
            block.start_address().into_usize(),
            block.size(),
            0,
        ),
        Err(PermissionDenied),
    );

    let pi = "https://en.wikipedia.org/wiki/Pi#/media/File:Pi_pie2.jpg";
    let user_memory = unsafe {
        process
            .lock()
            .address_space()
            .map_slice_zeroed::<u8>(pi.len(), USER_RW)
            .unwrap()
    };
    user_memory[.. pi.len()].copy_from_slice(pi.as_bytes());
    let block = Block::from_slice(user_memory);
    let result = log_value(
        process.lock(),
        info,
        block.start_address().into_usize(),
        pi.len(),
        3141592653589793238,
    );
    assert!(result.is_ok(), "expected Ok(_), got {result:?}");

    assert_eq!(
        log_value(process.lock(), info, 0, 0, 0),
        Err(PermissionDenied),
    );
    assert_eq!(
        log_value(process.lock(), info, 1, 0, 0),
        Err(PermissionDenied),
    );
    assert_eq!(
        log_value(process.lock(), info, 1, 1, 0),
        Err(PermissionDenied),
    );

    let result = log_value(process.lock(), info, user_pages().start(), 1, 0);
    assert!(
        result.is_err(),
        "expected Err(PermissionDenied) or Err(NoPage), got Ok",
    );
    assert!(
        result == Err(PermissionDenied) || result == Err(NoPage),
        "expected Err(PermissionDenied) or Err(NoPage), got {result:?}",
    );

    let invalid_utf8 = b"\xFF";
    user_memory[.. invalid_utf8.len()].copy_from_slice(invalid_utf8);
    let block = Block::from_slice(user_memory);
    assert_eq!(
        log_value(
            process.lock(),
            info,
            block.start_address().into_usize(),
            invalid_utf8.len(),
            0,
        ),
        Err(InvalidArgument),
    );

    for (address, size) in [
        (0x1_0000, 0xFFFF_FFFF_0000_0000),
        (0xFFFF_FFFF_FFFF_0000, 0x10_0000),
    ] {
        let result = log_value(process.lock(), info, address, size, 0);
        assert!(
            result == Err(InvalidArgument) || result == Err(Overflow),
            "expected Err(InvalidArgument) or Err(Overflow), got {result:?}",
        );
    }
}
