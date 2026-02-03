#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use x86_64::instructions::{
    self,
    interrupts,
};

use kernel::{
    Subsystems,
    log::info,
    memory::{
        BASE_ADDRESS_SPACE,
        test_scaffolding::phys2virt,
    },
    process::test_scaffolding::set_handler,
    smp::test_scaffolding::{
        cpu_id,
        init_smp,
    },
};

mod init;

init!(Subsystems::MEMORY);

#[test_case]
fn ap_init() {
    set_handler(ap_loop);

    let phys2virt = phys2virt(&BASE_ADDRESS_SPACE.lock());
    init_smp(phys2virt, Subsystems::SMP).unwrap();

    let cpu_count = 4;

    barrier(cpu_count);
    race();

    let racy_counter = unsafe { RACY_COUNTER };
    let race_free_counter = CPU_COUNTER * cpu_count;
    info!(racy_counter, race_free_counter);
    assert_ne!(racy_counter, race_free_counter);
}

fn ap_loop() {
    let cpu_count = 4;

    barrier(cpu_count);
    race();

    let cpu = cpu_id();
    info!(cpu, "AP halted");

    loop {
        interrupts::without_interrupts(instructions::hlt)
    }
}

fn barrier(cpu_count: usize) {
    let cpu = cpu_id();
    let cpus_waiting = BARRIER.fetch_add(1, Ordering::Relaxed);
    let last = cpus_waiting == cpu_count - 1;
    info!(cpu, cpu_count, cpus_waiting, last, "arrived at the barrier");

    loop {
        if BARRIER.load(Ordering::Relaxed) == cpu_count {
            info!(cpu, cpu_count, "all CPUs have arrived at the barrier");
            return;
        }

        instructions::hlt();
    }
}

fn race() {
    for _ in 0 .. CPU_COUNTER {
        unsafe {
            RACY_COUNTER += 1;
        }
    }
}

static BARRIER: AtomicUsize = AtomicUsize::new(0);
static mut RACY_COUNTER: usize = 0;

const CPU_COUNTER: usize = 5_000_000;
