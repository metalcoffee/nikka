#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use core::arch::asm;

use chrono::Duration;

use ku::{
    process::Syscall,
    time,
};

use lib::entry;

entry!(main);

fn main() {
    exit(7);
}

fn exit(code: usize) -> ! {
    // Wait for some PIT ticks so an interrupt will be pending on the return to the kernel mode.
    // This test is run with interrupts disabled.
    // If the kernel enables interrupts before switching the stack it will receive a Page Fault.
    time::delay(Duration::milliseconds(100));

    unsafe {
        asm!(
            "
            // Test that the kernel switches into its own stack.
            xor rsp, rsp

            syscall
            ",

            in("rax") usize::from(Syscall::Exit),
            in("rdi") code,

            options(noreturn),
        );
    }
}
