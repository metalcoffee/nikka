#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use core::{
    panic::PanicInfo,
    ptr::NonNull,
};

use bootloader::{
    BootInfo,
    entry_point,
};

use ku::sync;

use kernel::{
    log::info,
    trap::{
        TRAP_STATS,
        Trap,
    },
};

entry_point!(test_entry);

fn test_entry(boot_info: &'static BootInfo) -> ! {
    kernel::init(boot_info);
    test_main();
    panic!("should not return to test_entry()")
}

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    sync::start_panicking();

    let page_fault_count = TRAP_STATS[Trap::PageFault].count();
    info!(%panic_info, page_fault_count);
    if page_fault_count == 1 {
        kernel::pass_test()
    } else {
        kernel::fail_test(panic_info)
    }
}

#[test_case]
fn panic_in_logging() {
    let invalid_reference = unsafe { NonNull::<u8>::dangling().as_ref() };
    info!(invalid_reference);
}
