#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use lib::{
    entry,
    syscall,
};

entry!(main);

fn main() {
    loop {
        syscall::sched_yield();
    }
}
