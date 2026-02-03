#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![allow(clippy::unusual_byte_groupings)]
#![no_std]

use core::hint;

use x86::io;

pub trait Serial {
    fn new() -> Self;

    fn print_octet(
        &mut self,
        octet: u8,
    );
}

pub struct Com {}

impl Serial for Com {
    fn new() -> Self {
        const COM1_LSB: u16 = 0x03F8;
        const COM1_MSB: u16 = 0x03F9;
        const COM1_FIFO: u16 = 0x03FA;
        const COM1_LINE: u16 = 0x03FB;

        unsafe fn out_u16(value: u16) {
            unsafe {
                io::outb(COM1_LSB, value as u8);
                io::outb(COM1_MSB, (value >> 8) as u8);
            }
        }

        unsafe {
            // 1|0|001|0|11 = enable speed change|break disable|odd parity|1 stop bit|8 data bits
            io::outb(COM1_LINE, 0b_1_0_001_0_11);

            // (msb << 8) | lsb == 1.8432 MHz / (16 * speed_in_bauds) ==
            //   1843200 / (16 * speed_in_bauds) == 115200 / speed_in_bauds.
            // Standard speeds are (in bauds):
            //   50, 75, 100, 110, 200, 300, 600, 1200, 2400, 4800,
            //   9600, 19200, 38400, 57600, 115200.
            const BASE_NUMERATOR: u32 = 115200;
            const SPEED_IN_BAUDS: u32 = 9600;
            out_u16((BASE_NUMERATOR / SPEED_IN_BAUDS).try_into().expect("invalid speed"));

            io::outb(COM1_LINE, 0x0B);

            // Reset and clear buffers.
            io::outb(COM1_FIFO, 0x07);
        }

        Self {}
    }

    fn print_octet(
        &mut self,
        octet: u8,
    ) {
        fn transmitter_is_ready() -> bool {
            const COM1_LINE_STATUS_REGISTER: u16 = 0x03FD;
            const TRANSMITTER_HOLDING_REGISTER_EMPTY: u8 = 1 << 5;

            let status = unsafe { io::inb(COM1_LINE_STATUS_REGISTER) };

            status & TRANSMITTER_HOLDING_REGISTER_EMPTY != 0
        }

        const COM1_DATA: u16 = 0x03F8;

        while !transmitter_is_ready() {
            hint::spin_loop();
        }

        unsafe {
            io::outb(COM1_DATA, octet);
        }
    }
}
