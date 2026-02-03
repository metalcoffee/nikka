#![deny(warnings)]

use ku::memory::Virt;

#[test]
fn addr_formatting() {
    macro_rules! check_addr_formatting {
        ($addr:expr) => {
            let virt = Virt::new($addr).unwrap();
            assert_eq!(format!("{}", virt).replace("v", "x"), stringify!($addr));
        };
    }

    check_addr_formatting!(0x0);
    check_addr_formatting!(0x10);
    check_addr_formatting!(0x100);
    check_addr_formatting!(0x1000);
    check_addr_formatting!(0x1_0000);
    check_addr_formatting!(0x10_0000);
    check_addr_formatting!(0x100_0000);
    check_addr_formatting!(0x1000_0000);
    check_addr_formatting!(0x1_0000_0000);

    check_addr_formatting!(0x1_F000_0001);
    check_addr_formatting!(0x1_0F00_0010);
    check_addr_formatting!(0x1_00F0_0100);
    check_addr_formatting!(0x1_000F_1000);
}
