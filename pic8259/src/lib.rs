#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![allow(clippy::missing_safety_doc)]
#![allow(clippy::unusual_byte_groupings)]
#![no_std]

use x86::io;

const INTERRUPT_LINE_COUNT: u8 = 8;
pub const PIC_INTERRUPT_COUNT: u8 = INTERRUPT_LINE_COUNT * 2;

pub unsafe fn init(first_interrupt_index: u8) {
    const PIC0_DATA: u16 = PIC0_COMMAND + 1;
    const PIC1_DATA: u16 = PIC1_COMMAND + 1;

    const ICW1_USE_ICW4: u8 = 0b_1 << 0;
    const ICW1_CASCADE: u8 = 0b_0 << 1;
    const ICW1_LEVEL_TRIGGERED: u8 = 0b_0 << 3;
    const ICW1_MANDATORY_BITS: u8 = 0b_1 << 4;
    const ICW1: u8 = ICW1_USE_ICW4 | ICW1_CASCADE | ICW1_LEVEL_TRIGGERED | ICW1_MANDATORY_BITS;

    unsafe {
        io::outb(PIC0_COMMAND, ICW1);
        io::outb(PIC1_COMMAND, ICW1);
    }

    let icw2_for_pic0: u8 = first_interrupt_index;
    let icw2_for_pic1: u8 = first_interrupt_index + INTERRUPT_LINE_COUNT;

    unsafe {
        io::outb(PIC0_DATA, icw2_for_pic0);
        io::outb(PIC1_DATA, icw2_for_pic1);
    }

    const CASCADE_LINE: u8 = 2;
    const CASCADE_LINES_BITMASK: u8 = 1 << CASCADE_LINE;
    const ICW3_FOR_PIC0: u8 = CASCADE_LINES_BITMASK;
    const ICW3_FOR_PIC1: u8 = CASCADE_LINE;

    unsafe {
        io::outb(PIC0_DATA, ICW3_FOR_PIC0);
        io::outb(PIC1_DATA, ICW3_FOR_PIC1);
    }

    // With an automatic End Of Interrupt there is no need
    // for sending an explicit End Of Interrupt.
    // This simplifies and speeds up interrupt routines.
    // But in this mode interrupts loose priority and can be self nested.
    // It should not matter because
    //   - the CPUs are faster then perepherals now,
    //   - interrupt routines should hand off their work to modules,
    //   - modules should be properly prioritized themselves,
    //   - we plan to use IO APIC anyway.
    const ICW4_MANDATORY_BITS: u8 = 0b_1 << 0;
    const ICW4_AUTOMATIC_END_OF_INTERRUPT: u8 = 0b_1 << 1;
    const ICW4_UNBUFFERED_MODE: u8 = 0b_00 << 2;
    const ICW4_NORMAL_NESTED_MODE: u8 = 0b_0 << 4;
    const ICW4: u8 = ICW4_MANDATORY_BITS |
        ICW4_AUTOMATIC_END_OF_INTERRUPT |
        ICW4_UNBUFFERED_MODE |
        ICW4_NORMAL_NESTED_MODE;

    unsafe {
        io::outb(PIC0_DATA, ICW4);
        io::outb(PIC1_DATA, ICW4);
    }

    const DISABLED_INTERRUPT_LINES_BITMASK: u8 = 0b_0000_0000;
    const OCW1: u8 = DISABLED_INTERRUPT_LINES_BITMASK;

    unsafe {
        io::outb(PIC0_DATA, OCW1);
        io::outb(PIC1_DATA, OCW1);
    }
}

pub unsafe fn end_of_interrupt(pic_interrupt_number: usize) {
    const EOI: u8 = 0x20;

    if pic_interrupt_number >= 8 {
        unsafe {
            io::outb(PIC1_COMMAND, EOI);
        }
    }

    unsafe {
        io::outb(PIC0_COMMAND, EOI);
    }
}

const PIC0_COMMAND: u16 = 0x20;
const PIC1_COMMAND: u16 = 0xA0;
