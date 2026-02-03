#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::vec::Vec;

use kernel::{
    Subsystems,
    error::Error::NoPage,
    log::info,
    memory::{
        BASE_ADDRESS_SPACE,
        FULL_ACCESS,
        KERNEL_RW,
        Page,
        test_scaffolding::translate,
    },
    smp::test_scaffolding::{
        cpu_count,
        cpu_id,
        id,
        kernel_stack_zones,
    },
};

mod init;

init!(Subsystems::MEMORY | Subsystems::LOCAL_APIC | Subsystems::CPUS);

#[test_case]
fn initialized() {
    let cpu = cpu_id();
    let local_apic_id = id();
    info!(cpu, local_apic_id);

    assert_eq!(cpu, local_apic_id);
}

#[test_case]
fn kernel_stacks() {
    let mut stacks = Vec::with_capacity(cpu_count());

    for cpu in 0 .. cpu_count() {
        let (stack_guard, stack) = kernel_stack_zones(cpu);

        info!(cpu, %stack_guard, %stack);

        stacks.push(stack);

        assert_eq!(stack.start_address().into_usize() % Page::SIZE, 0);

        let mut address_space = BASE_ADDRESS_SPACE.lock();

        let stack_mapping_error = "kernel stack is not mapped";

        for stack_page in stack.enclosing() {
            let stack_pte =
                translate(&mut address_space, stack_page.address()).expect(stack_mapping_error);
            let stack_frame = stack_pte.frame().expect(stack_mapping_error);
            let stack_flags = stack_pte.flags();

            if stack_page.address() == stack.start_address() {
                info!(cpu, %stack_page, ?stack_frame, ?stack_flags);
            }

            assert!(stack_pte.is_present(), "{stack_mapping_error}");
            assert_ne!(stack_page, Page::default(), "{stack_mapping_error}");
            assert_eq!(
                stack_flags & FULL_ACCESS,
                KERNEL_RW,
                "wrong flags for the stack",
            );
        }

        if cpu != cpu_id().into() {
            unsafe {
                stack.try_into_mut_slice::<usize>().unwrap().fill(0);
            }
        }

        for stack_guard_page in stack_guard.enclosing() {
            let stack_guard_frame = translate(&mut address_space, stack_guard_page.address())
                .and_then(|pte| pte.frame());

            info!(cpu, ?stack_guard_frame);
            assert_eq!(
                stack_guard_frame,
                Err(NoPage),
                "kernel stack guard is not unmapped",
            );
        }
    }

    introsort::sort_by(stacks.as_mut_slice(), &|a, b| {
        a.start_address().cmp(&b.start_address())
    });

    for (stack_a, stack_b) in stacks.iter().zip(&stacks[1 ..]) {
        info!(%stack_a, %stack_b);
        assert!(stack_a.end_address().unwrap() < stack_b.start_address());
    }
}
