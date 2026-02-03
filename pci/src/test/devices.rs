extern crate alloc;

use alloc::{
    vec,
    vec::Vec,
};

use hex_literal::hex;

use ku::memory::{
    Block,
    MiB,
};

use crate::{
    Bar,
    Kind,
    bar::Width,
};

use super::MockDevice;

pub fn all() -> Vec<MockDevice> {
    normal().into_iter().chain(bridge()).collect()
}

pub fn normal() -> Vec<MockDevice> {
    vec![normal_0(), normal_1(), normal_2()]
}

pub fn bridge() -> Vec<MockDevice> {
    vec![bridge_0(), bridge_1(), bridge_2()]
}

/// 01:00.0 VGA compatible controller: NVIDIA Corporation GF114 [GeForce GTX 560] (rev a1) (prog-if 00 [VGA controller])
///    Subsystem: ASUSTeK Computer Inc. GF114 [GeForce GTX 560]
///    Flags: bus master, fast devsel, latency 0, IRQ 33
///    Memory at f4000000 (32-bit, non-prefetchable) [size=32M]
///    Memory at e8000000 (64-bit, prefetchable) [size=128M]
///    Memory at f0000000 (64-bit, prefetchable) [size=64M]
///    I/O ports at e000 [size=128]
///    Expansion ROM at 000c0000 [disabled] [size=128K]
///    Capabilities: [60] Power Management version 3
///    Capabilities: [68] MSI: Enable+ Count=1/1 Maskable- 64bit+
///    Capabilities: [78] Express Endpoint, MSI 00
///    Capabilities: [b4] Vendor Specific Information: Len=14 <?>
///    Capabilities: [100] Virtual Channel
///    Capabilities: [128] Power Budgeting <?>
///    Capabilities: [600] Vendor Specific Information: ID=0001 Rev=1 Len=024 <?>
///    Kernel driver in use: nouveau
///    Kernel modules: nvidiafb, nouveau
fn normal_0() -> MockDevice {
    let config_space = hex!(
        r#"
        DE 10 01 12 07 04 10 00 A1 00 00 03 10 00 80 00
        00 00 00 F4 0C 00 00 E8 00 00 00 00 0C 00 00 F0
        00 00 00 00 01 E0 00 00 00 00 00 00 43 10 B5 83
        00 00 00 F6 60 00 00 00 00 00 00 00 0B 01 00 00
        43 10 B5 83 00 00 00 00 00 00 00 00 00 00 00 00
        01 00 00 00 01 00 00 00 CE D6 23 00 00 00 00 00
        01 68 03 00 08 00 00 00 05 78 81 00 00 40 E0 FE
        00 00 00 00 22 00 00 00 10 B4 02 00 A0 8D 2C 01
        00 29 00 00 01 2D 05 00 40 00 01 11 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 10 00 00 00
        00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00
        00 00 00 00 09 00 14 01 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        "#
    );

    let bars = unsafe {
        [
            Some(Bar::Memory {
                block: Block::from_index_count_unchecked(0xF400_0000, 32 * MiB),
                prefetchable: false,
                width: Width::Memory32,
            }),
            Some(Bar::Memory {
                block: Block::from_index_count_unchecked(0xE800_0000, 128 * MiB),
                prefetchable: true,
                width: Width::Memory64,
            }),
            None,
            Some(Bar::Memory {
                block: Block::from_index_count_unchecked(0xF000_0000, 64 * MiB),
                prefetchable: true,
                width: Width::Memory64,
            }),
            None,
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xE000, 128),
            }),
        ]
    };

    MockDevice::new(
        config_space,
        bars,
        "NVIDIA Corporation",
        "GF114 [GeForce GTX 560]",
        0xA1,
        Some(("ASUSTeK Computer Inc.", 0x83B5)),
        "Display controller",
        "VGA compatible controller",
        0x00,
        true,
        Kind::Normal { bars },
    )
}

/// 00:1f.2 IDE interface: Intel Corporation 7 Series/C210 Series Chipset Family 4-port SATA Controller [IDE mode] (rev 04) (prog-if 8f [PCI native mode controller, supports both channels switched to ISA compatibility mode, supports bus mastering])
///    Subsystem: Gigabyte Technology Co., Ltd 7 Series/C210 Series Chipset Family 4-port SATA Controller [IDE mode]
///    Flags: bus master, 66MHz, medium devsel, latency 0, IRQ 19
///    I/O ports at f0d0 [size=8]
///    I/O ports at f0c0 [size=4]
///    I/O ports at f0b0 [size=8]
///    I/O ports at f0a0 [size=4]
///    I/O ports at f090 [size=16]
///    I/O ports at f080 [size=16]
///    Capabilities: [70] Power Management version 3
///    Capabilities: [b0] PCI Advanced Features
///    Kernel driver in use: ata_piix
///    Kernel modules: pata_acpi
fn normal_1() -> MockDevice {
    let config_space = hex!(
        r#"
        86 80 00 1E 07 00 B0 02 04 8F 01 01 00 00 00 00
        D1 F0 00 00 C1 F0 00 00 B1 F0 00 00 A1 F0 00 00
        91 F0 00 00 81 F0 00 00 00 00 00 00 58 14 05 B0
        00 00 00 00 70 00 00 00 00 00 00 00 05 02 00 00
        00 80 00 80 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        01 B0 03 00 08 00 00 00 00 00 00 00 00 00 00 00
        05 70 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 36 09 89 83 01 00 36 08 42 5C 01 00 00 00 00
        E0 00 00 00 39 00 00 00 00 00 00 00 00 00 00 00
        13 00 06 03 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 0D 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 87 0F 04 08 00 00 00 00
        "#
    );

    let bars = unsafe {
        [
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF0D0, 8),
            }),
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF0C0, 4),
            }),
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF0B0, 8),
            }),
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF0A0, 4),
            }),
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF090, 16),
            }),
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF080, 16),
            }),
        ]
    };

    MockDevice::new(
        config_space,
        bars,
        "Intel Corporation",
        "7 Series/C210 Series Chipset Family 4-port SATA Controller [IDE mode]",
        0x04,
        Some(("Gigabyte Technology Co., Ltd", 0xB005)),
        "Mass storage controller",
        "IDE interface",
        0x8F,
        false,
        Kind::Normal { bars },
    )
}

/// 00:1f.3 SMBus: Intel Corporation 7 Series/C216 Chipset Family SMBus Controller (rev 04)
///    Subsystem: Gigabyte Technology Co., Ltd 7 Series/C216 Chipset Family SMBus Controller
///    Flags: medium devsel, IRQ 18
///    Memory at f6315000 (64-bit, non-prefetchable) [size=256]
///    I/O ports at f000 [size=32]
///    Kernel driver in use: i801_smbus
///    Kernel modules: i2c_i801
fn normal_2() -> MockDevice {
    let config_space = hex!(
        r#"
        86 80 22 1e 03 00 80 02 04 00 05 0c 00 00 00 00
        04 50 31 f6 00 00 00 00 00 00 00 00 00 00 00 00
        01 f0 00 00 00 00 00 00 00 00 00 00 58 14 01 50
        00 00 00 00 00 00 00 00 00 00 00 00 0b 03 00 00
        01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        03 04 04 00 00 00 08 08 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        04 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 87 0f 04 08 00 00 00 00
        "#
    );

    let bars = unsafe {
        [
            Some(Bar::Memory {
                block: Block::from_index_count_unchecked(0xF631_5000, 256),
                prefetchable: false,
                width: Width::Memory64,
            }),
            None,
            None,
            None,
            Some(Bar::Port {
                block: Block::from_index_count_unchecked(0xF000, 32),
            }),
            None,
        ]
    };

    MockDevice::new(
        config_space,
        bars,
        "Intel Corporation",
        "7 Series/C216 Chipset Family SMBus Controller",
        0x04,
        Some(("Gigabyte Technology Co., Ltd", 0x5001)),
        "Serial bus controller",
        "SMBus",
        0x00,
        false,
        Kind::Normal { bars },
    )
}

/// 00:01.0 PCI bridge: Intel Corporation Xeon E3-1200 v2/3rd Gen Core processor PCI Express Root Port (rev 09) (prog-if 00 [Normal decode])
///    Flags: bus master, fast devsel, latency 0, IRQ 24
///    Bus: primary=00, secondary=01, subordinate=01, sec-latency=0
///    I/O behind bridge: 0000e000-0000efff [size=4K]
///    Memory behind bridge: f4000000-f60fffff [size=33M]
///    Prefetchable memory behind bridge: 00000000e8000000-00000000f3ffffff [size=192M]
///    Capabilities: [88] Subsystem: Gigabyte Technology Co., Ltd Xeon E3-1200 v2/3rd Gen Core processor PCI Express Root Port
///    Capabilities: [80] Power Management version 3
///    Capabilities: [90] MSI: Enable+ Count=1/1 Maskable- 64bit-
///    Capabilities: [a0] Express Root Port (Slot+), MSI 00
///    Capabilities: [100] Virtual Channel
///    Capabilities: [140] Root Complex Link
///    Capabilities: [d94] Secondary PCI Express
///    Kernel driver in use: pcieport
fn bridge_0() -> MockDevice {
    let config_space = hex!(
        r#"
        86 80 51 01 07 04 10 00 09 00 04 06 10 00 81 00
        00 00 00 00 00 00 00 00 00 01 01 00 E0 E0 00 20
        00 F4 00 F6 01 E8 F1 F3 00 00 00 00 00 00 00 00
        00 00 00 00 88 00 00 00 00 00 00 00 0B 01 1A 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 0A
        01 90 03 C8 08 00 00 00 0D 80 00 00 58 14 00 50
        05 A0 01 00 00 40 E0 FE 20 00 00 00 00 00 00 00
        10 00 42 01 01 80 00 00 00 00 00 00 03 A1 61 02
        40 00 01 51 80 25 0C 00 00 00 40 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 0E 00 00 00
        43 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 01 00 00 00 00 00 01 00 10 00
        "#
    );

    let bars = [None; 6];

    MockDevice::new(
        config_space,
        bars,
        "Intel Corporation",
        "Xeon E3-1200 v2/3rd Gen Core processor PCI Express Root Port",
        0x09,
        None,
        "Bridge",
        "PCI bridge",
        0x00,
        true,
        Kind::PciPciBridge {
            bars: [None; 2],
            primary: 0,
            secondary: 1,
            subordinate: 1,
            io_behind_bridge: Block::from_index(0xE000, 0xF000).unwrap(),
            memory_behind_bridge: Block::from_index(0xF400_0000, 0xF610_0000).unwrap(),
            prefetchable_memory_behind_bridge: Block::from_index(0xE800_0000, 0xF400_0000).unwrap(),
        },
    )
}

/// 00:1c.0 PCI bridge: Intel Corporation 7 Series/C216 Chipset Family PCI Express Root Port 1 (rev c4) (prog-if 00 [Normal decode])
///    Flags: bus master, fast devsel, latency 0, IRQ 16
///    Bus: primary=00, secondary=02, subordinate=02, sec-latency=0
///    I/O behind bridge: [disabled]
///    Memory behind bridge: [disabled]
///    Prefetchable memory behind bridge: [disabled]
///    Capabilities: [40] Express Root Port (Slot+), MSI 00
///    Capabilities: [80] MSI: Enable- Count=1/1 Maskable- 64bit-
///    Capabilities: [90] Subsystem: Gigabyte Technology Co., Ltd 7 Series/C216 Chipset Family PCI Express Root Port 1
///    Capabilities: [a0] Power Management version 2
///    Kernel driver in use: pcieport
fn bridge_1() -> MockDevice {
    let config_space = hex!(
        r#"
        86 80 10 1E 07 00 10 00 C4 00 04 06 10 00 81 00
        00 00 00 00 00 00 00 00 00 02 02 00 F0 00 00 20
        F0 FF 00 00 F1 FF 01 00 00 00 00 00 00 00 00 00
        00 00 00 00 40 00 00 00 00 00 00 00 0B 01 12 00
        10 80 42 01 00 80 00 00 00 00 10 00 12 40 11 01
        00 00 01 18 00 B2 04 00 00 00 00 00 00 00 00 00
        00 00 00 00 16 00 00 00 00 00 00 00 00 00 00 00
        01 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00
        05 90 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        0D A0 00 00 58 14 01 50 00 00 00 00 00 00 00 00
        01 00 02 C8 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 01 02 0B 00 00 00 80 11 81 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 87 0F 04 08 00 00 00 00
        "#
    );

    let bars = [None; 6];

    MockDevice::new(
        config_space,
        bars,
        "Intel Corporation",
        "7 Series/C216 Chipset Family PCI Express Root Port 1",
        0xC4,
        None,
        "Bridge",
        "PCI bridge",
        0x00,
        true,
        Kind::PciPciBridge {
            bars: [None; 2],
            primary: 0,
            secondary: 2,
            subordinate: 2,
            io_behind_bridge: Block::default(),
            memory_behind_bridge: Block::default(),
            prefetchable_memory_behind_bridge: Block::default(),
        },
    )
}

/// 00:1c.2 PCI bridge: Intel Corporation 7 Series/C210 Series Chipset Family PCI Express Root Port 3 (rev c4) (prog-if 00 [Normal decode])
///    Flags: bus master, fast devsel, latency 0, IRQ 18
///    Bus: primary=00, secondary=03, subordinate=03, sec-latency=0
///    I/O behind bridge: 0000d000-0000dfff [size=4K]
///    Memory behind bridge: f6200000-f62fffff [size=1M]
///    Prefetchable memory behind bridge: [disabled]
///    Capabilities: [40] Express Root Port (Slot+), MSI 00
///    Capabilities: [80] MSI: Enable- Count=1/1 Maskable- 64bit-
///    Capabilities: [90] Subsystem: Gigabyte Technology Co., Ltd 7 Series/C210 Series Chipset Family PCI Express Root Port 3
///    Capabilities: [a0] Power Management version 2
///    Kernel driver in use: pcieport
fn bridge_2() -> MockDevice {
    let config_space = hex!(
        r#"
        86 80 14 1E 07 00 10 00 C4 00 04 06 10 00 81 00
        00 00 00 00 00 00 00 00 00 03 03 00 D0 D0 00 20
        20 F6 20 F6 F1 FF 01 00 00 00 00 00 00 00 00 00
        00 00 00 00 40 00 00 00 00 00 00 00 0B 03 12 00
        10 80 42 01 00 80 00 00 00 00 10 00 12 40 11 03
        00 00 11 70 00 B2 14 00 00 00 40 00 00 00 00 00
        00 00 00 00 16 00 00 00 00 00 00 00 00 00 00 00
        02 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00
        05 90 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        0D A0 00 00 58 14 01 50 00 00 00 00 00 00 00 00
        01 00 02 C8 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 01 02 0B 00 00 00 80 11 81 00 00 00 00
        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
        00 00 00 00 00 00 00 00 87 0F 04 08 00 00 00 00
        "#
    );

    let bars = [None; 6];

    MockDevice::new(
        config_space,
        bars,
        "Intel Corporation",
        "7 Series/C210 Series Chipset Family PCI Express Root Port 3",
        0xC4,
        None,
        "Bridge",
        "PCI bridge",
        0x00,
        true,
        Kind::PciPciBridge {
            bars: [None; 2],
            primary: 0,
            secondary: 3,
            subordinate: 3,
            io_behind_bridge: Block::from_index(0xD000, 0xE000).unwrap(),
            memory_behind_bridge: Block::from_index(0xF620_0000, 0xF630_0000).unwrap(),
            prefetchable_memory_behind_bridge: Block::default(),
        },
    )
}
