#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use core::arch::asm;

use lib::entry;

entry!(main);

#[allow(named_asm_labels)]
fn main() {
    // The repeated `je`/`jne` serve as a workaround for what appears to be instruction fusion:
    // https://stackoverflow.com/a/56413946/19014121 .
    // Without them, interrupts only occur on the `cmp` instruction,
    // and never between `cmp` and `je`/`jne`.
    unsafe {
        asm!("
            mov rax, {rax}
            mov rbx, {rbx}
            mov rcx, {rcx}
            mov rdx, {rdx}
            mov rdi, {rdi}
            mov rsi, {rsi}
            mov rsp, {rsp}
            mov rbp, {rbp}
            mov r8, {r8}
            mov r9, {r9}
            mov r10, {r10}
            mov r11, {r11}
            mov r12, {r12}
            mov r13, {r13}
            mov r14, {r14}
            mov r15, {r15}

        loop:

            cmp rax, {rax}
            jne fail
            jne fail
            cmp rax, {rax} + 1
            je fail
            je fail

            cmp rbx, {rbx}
            jne fail
            jne fail
            cmp rbx, {rbx} + 1
            je fail
            je fail

            cmp rcx, {rcx}
            jne fail
            jne fail
            cmp rcx, {rcx} + 1
            je fail
            je fail

            cmp rdx, {rdx}
            jne fail
            jne fail
            cmp rdx, {rdx} + 1
            je fail
            je fail

            cmp rdi, {rdi}
            jne fail
            jne fail
            cmp rdi, {rdi} + 1
            je fail
            je fail

            cmp rsi, {rsi}
            jne fail
            jne fail
            cmp rsi, {rsi} + 1
            je fail
            je fail

            cmp rsp, {rsp}
            jne fail
            jne fail
            cmp rsp, {rsp} + 1
            je fail
            je fail

            cmp rbp, {rbp}
            jne fail
            jne fail
            cmp rbp, {rbp} + 1
            je fail
            je fail

            cmp r8, {r8}
            jne fail
            jne fail
            cmp r8, {r8} + 1
            je fail
            je fail

            cmp r9, {r9}
            jne fail
            jne fail
            cmp r9, {r9} + 1
            je fail
            je fail

            cmp r10, {r10}
            jne fail
            jne fail
            cmp r10, {r10} + 1
            je fail
            je fail

            cmp r11, {r11}
            jne fail
            jne fail
            cmp r11, {r11} + 1
            je fail
            je fail

            cmp r12, {r12}
            jne fail
            jne fail
            cmp r12, {r12} + 1
            je fail
            je fail

            cmp r13, {r13}
            jne fail
            jne fail
            cmp r13, {r13} + 1
            je fail
            je fail

            cmp r14, {r14}
            jne fail
            jne fail
            cmp r14, {r14} + 1
            je fail
            je fail

            cmp r15, {r15}
            jne fail
            jne fail
            cmp r15, {r15} + 1
            je fail
            je fail

            jmp loop

        fail:
            xor rsp, rsp
            add rsp, 3
            mov [rsp], rsp
            ",

            rax = const 0x0123,
            rbx = const 0x4567,
            rcx = const 0x89AB,
            rdx = const 0xCDEF,
            rdi = const 0x1011,
            rsi = const 0x1213,
            rsp = const 0x1415,
            rbp = const 0x1617,
            r8 = const 0x1819,
            r9 = const 0x1A1B,
            r10 = const 0x1C1D,
            r11 = const 0x1E1F,
            r12 = const 0x2021,
            r13 = const 0x2223,
            r14 = const 0x2425,
            r15 = const 0x2627,

            options(noreturn),
        );
    }
}
