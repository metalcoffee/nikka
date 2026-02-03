#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use kernel::{
    Subsystems,
    log::info,
    memory::{
        BASE_ADDRESS_SPACE,
        FULL_ACCESS,
        KERNEL_MMIO,
        Phys,
        test_scaffolding::translate,
    },
    smp::test_scaffolding::{
        id,
        local_apic,
    },
};

mod init;

init!(Subsystems::MEMORY | Subsystems::LOCAL_APIC);

#[test_case]
fn mapped_properly() {
    let local_apic = local_apic();
    let expected_local_apic_address = Phys::new(0xFEE00000).unwrap();

    let mapping_error = "Local APIC is not mapped";
    let pte = *translate(&mut BASE_ADDRESS_SPACE.lock(), local_apic).expect(mapping_error);
    let frame = pte.frame().expect(mapping_error);
    let flags = pte.flags();

    info!(%local_apic, ?frame, ?flags, "Local APIC");

    assert!(pte.is_present(), "{}", mapping_error);
    assert_eq!(
        frame.address(),
        expected_local_apic_address,
        "wrong physical address for the Local APIC",
    );
    assert_eq!(
        flags & (KERNEL_MMIO | FULL_ACCESS),
        KERNEL_MMIO,
        "wrong flags for the Local APIC virtual page",
    );

    info!(cpu = id());
}
