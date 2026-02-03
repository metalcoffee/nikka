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
    unsafe {
        asm!(
            "
            loop:
            jmp loop
            "
        );
    }
}
