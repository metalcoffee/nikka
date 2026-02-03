#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use core::arch::asm;

use chrono::Duration;

use ku::time;

use lib::entry;

entry!(main);

fn main() {
    // Wait for some PIT ticks so an interrupt will be pending on the return to the kernel mode.
    // This test is run with interrupts disabled.
    // If the kernel enables interrupts before switching the stack it will receive a Page Fault.
    time::delay(Duration::milliseconds(100));

    unsafe {
        asm!(
            "
            // Test that the kernel stores the user mode context properly.
            mov rax, 77701
            mov rbx, 77702
            mov rcx, 77703
            mov rdx, 77704
            mov rdi, 77705
            mov rsi, 77706
            mov rbp, 77707
            mov r8, 77708
            mov r9, 77709
            mov r10, 77710
            mov r11, 77711
            mov r12, 77712
            mov r13, 77713
            mov r14, 77714
            mov r15, 77715

            // Test that the kernel switches into its own stack.
            xor rsp, rsp

            // Test that the user mode Page Fault is handled.
            mov rsp, [rsp]
            ",
            options(noreturn),
        );
    }
}
