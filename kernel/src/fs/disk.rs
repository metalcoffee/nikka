use core::{
    arch::asm,
    hint,
    mem,
    ops::Range,
};

use bitflags::bitflags;
use chrono::Duration;
use derive_more::Display;
use static_assertions::const_assert_eq;
use x86::io;

use ku::{
    error::{
        Error::{
            Medium,
            NoDisk,
            Timeout,
        },
        Result,
    },
    log::{
        error,
        trace,
    },
    memory::size,
    time,
};

use super::block_cache::SECTORS_PER_BLOCK;

/// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA)--диск.
#[derive(Clone, Copy, Debug, Display)]
#[display("{{ id: {}, io_port: {:#04X}, io_disk: {} }}", id, io_port, io_disk)]
pub(super) struct Disk {
    /// Идентификатор диска --- `0..4`.
    id: u8,

    /// Идентификатор диска,
    /// передаваемый в [порт ввода--вывода](https://wiki.osdev.org/Port_IO) при операциях с ним.
    io_disk: u8,

    /// Базовый [порт ввода--вывода](https://wiki.osdev.org/Port_IO) для операций с диском.
    io_port: u16,
}

impl Disk {
    /// Возвращает диск с заданным `id`.
    pub(super) fn new(id: usize) -> Result<Self> {
        let id = id.try_into().map_err(|_| NoDisk)?;

        Ok(Self {
            io_port: Self::io_port(id)?,
            io_disk: Self::io_disk(id),
            id,
        })
    }

    /// Записывает содержимое кэша диска на физический носитель.
    pub(super) fn flush(&self) -> Result<()> {
        let result = unsafe { self.send_command(Command::FLUSH_CACHE, 0) };

        if result.is_err() {
            error!(sector = self.read_sector_number(), "flush failed");
        }

        result
    }

    /// Читает с диска диапазон секторов `sectors` размера [`SECTOR_SIZE`]
    /// в буфер `buffer` методом
    /// [программного ввода--вывода](https://en.wikipedia.org/wiki/Programmed_input%E2%80%93output).
    pub(super) fn pio_read(
        &self,
        sectors: Range<usize>,
        buffer: &mut [u32],
    ) -> Result<()> {
        assert_eq!(mem::size_of_val(buffer), sectors.len() * SECTOR_SIZE);

        unsafe {
            self.send_rw_command(Command::READ, sectors.start, sectors.len())?;
        }

        for sector in buffer.chunks_mut(SECTOR_SIZE / mem::size_of::<u32>()) {
            self.wait_ready()?;

            unsafe {
                ins32(self.io_port, sector);
            }
        }

        Ok(())
    }

    /// Количество блоков файловой системы, которые вмещает диск.
    pub(super) fn block_count(&self) -> Result<usize> {
        unsafe { self.send_command(Command::READ_NATIVE_MAX_ADDRESS, LBA)? };

        let max_sector_number = self.read_sector_number();

        Ok((max_sector_number + 1) / SECTORS_PER_BLOCK)
    }

    /// Записывает на диск диапазон секторов `sectors` размера [`SECTOR_SIZE`]
    /// из буфера `buffer` методом
    /// [программного ввода--вывода](https://en.wikipedia.org/wiki/Programmed_input%E2%80%93output).
    pub(super) fn pio_write(
        &self,
        sectors: Range<usize>,
        buffer: &[u32],
    ) -> Result<()> {
        assert_eq!(mem::size_of_val(buffer), sectors.len() * SECTOR_SIZE);

        unsafe {
            self.send_rw_command(Command::WRITE, sectors.start, sectors.len())?;
        }

        for sector in buffer.chunks(SECTOR_SIZE / mem::size_of::<u32>()) {
            self.wait_ready()?;

            unsafe {
                outs32(self.io_port, sector);
            }
        }

        Ok(())
    }

    /// Посылает в диск команду `command` для
    /// [программного ввода--вывода](https://en.wikipedia.org/wiki/Programmed_input%E2%80%93output)
    /// `sector_count` секторов начиная с сектора номер `start_sector`.
    unsafe fn send_command(
        &self,
        command: Command,
        argument: u8,
    ) -> Result<()> {
        self.wait_ready()?;

        unsafe {
            io::outb(
                self.io_port + 6,
                RESERVED_SHOULD_BE_ONES | argument | (self.io_disk << 4),
            );
            io::outb(self.io_port + 7, command.bits());
        }

        self.wait_ready()
    }

    /// Посылает в диск команду `command` для
    /// [программного ввода--вывода](https://en.wikipedia.org/wiki/Programmed_input%E2%80%93output)
    /// `sector_count` секторов начиная с сектора номер `start_sector`.
    unsafe fn send_rw_command(
        &self,
        command: Command,
        start_sector: usize,
        sector_count: usize,
    ) -> Result<()> {
        assert!(sector_count < (1 << 8));

        self.wait_ready()?;

        unsafe {
            io::outb(self.io_port + 2, sector_count as u8);
            self.write_sector_number(start_sector);
            io::outb(self.io_port + 7, command.bits());
        }

        Ok(())
    }

    /// Читает из регистров контроллера номер сектора.
    fn read_sector_number(&self) -> usize {
        self.sector_number_ports()
            .rev()
            .map(|port| unsafe { io::inb(port) })
            .fold(0, |sector_number, x| (sector_number << 8) | size::from(x)) &
            SECTOR_NUMBER_MASK
    }

    /// Записывает в регистры контроллера номер сектора.
    unsafe fn write_sector_number(
        &self,
        sector: usize,
    ) {
        assert_eq!((sector & !SECTOR_NUMBER_MASK), 0);

        let value = u32::try_from(sector).expect("sector number is too big") |
            (u32::from(RESERVED_SHOULD_BE_ONES | LBA | (self.io_disk << 4)) << 24);

        for (port, x) in self.sector_number_ports().zip(value.to_le_bytes()) {
            unsafe {
                io::outb(port, x);
            }
        }
    }

    /// Ожидает готовности диска к приёму команды.
    fn wait_ready(&self) -> Result<()> {
        bitflags! {
            /// Регистр статуса.
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            struct Status: u8 {
                const ERROR = 1 << 0;
                const FAILURE = 1 << 5;
                const READY = 1 << 6;
                const BUSY = 1 << 7;
            }
        }

        let mut iterations = 1;
        let mut last_status = None;
        let start = time::timer();
        let timeout = Duration::seconds(TIMEOUT_IN_SECONDS);

        while last_status.is_none() || !start.has_passed(timeout) {
            let status = Status::from_bits_truncate(unsafe { io::inb(self.io_port + 7) });
            last_status = Some(status);

            if status.contains(Status::READY) && !status.contains(Status::BUSY) {
                trace!(elapsed = %start.elapsed(), iterations, ?status, "waited for IDE");

                return if status.intersects(Status::ERROR | Status::FAILURE) {
                    Err(Medium)
                } else {
                    Ok(())
                };
            }

            hint::spin_loop();
            iterations += 1;
        }

        error!(elapsed = %start.elapsed(), iterations, ?last_status, "timeout waiting for IDE");

        Err(Timeout)
    }

    /// Базовый [порт ввода--вывода](https://wiki.osdev.org/Port_IO)
    /// для операций с диском имеющим порядковый номер `id`.
    fn io_port(id: u8) -> Result<u16> {
        /// Базовый порт первого и второго
        /// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA)--диска.
        const ATA0_BASE_PORT: u16 = 0x01F0;

        /// Базовый порт третьего и четвёртого
        /// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA)--диска.
        const ATA1_BASE_PORT: u16 = 0x0170;

        match id / 2 {
            0 => Ok(ATA0_BASE_PORT),
            1 => Ok(ATA1_BASE_PORT),
            _ => Err(NoDisk),
        }
    }

    /// Идентификатор диска,
    /// передаваемый в [порт ввода--вывода](https://wiki.osdev.org/Port_IO)
    /// для операций с диском имеющим порядковый номер `id`.
    fn io_disk(id: u8) -> u8 {
        id % 2
    }

    /// Возвращает диапазон портов, которые предназначены для чтения или записи номера сектора.
    fn sector_number_ports(&self) -> Range<u16> {
        (self.io_port + 3) .. (self.io_port + 7)
    }
}

/// Записывает в порт ввода--вывода номер `port` данные из буфера `buffer`.
unsafe fn outs32(
    port: u16,
    buffer: &[u32],
) {
    unsafe {
        asm!(
            "
            rep outs dx, LONG PTR [rsi]
            ",
            in("rdx") port,
            in("rcx") buffer.len(),
            in("rsi") buffer.as_ptr(),
            lateout("rcx") _,
            options(nostack),
        );
    }
}

/// Читает из порта ввода--вывода номер `port` данные в буфер `buffer`.
unsafe fn ins32(
    port: u16,
    buffer: &mut [u32],
) {
    unsafe {
        asm!(
            "
            rep ins LONG PTR [rdi], dx
            ",
            in("rdx") port,
            in("rcx") buffer.len(),
            in("rdi") buffer.as_mut_ptr(),
            lateout("rcx") _,
            options(nostack),
        );
    }
}

/// Размер сектора [PATA](https://en.wikipedia.org/wiki/Parallel_ATA)--диска.
pub(super) const SECTOR_SIZE: usize = 1 << 9;

/// Тайм-аут ожидания готовности диска к приёму команды в секундах.
const TIMEOUT_IN_SECONDS: i64 = 10;

const_assert_eq!(SECTOR_SIZE % mem::size_of::<u32>(), 0);

bitflags! {
    /// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA)--команда работы с диском.
    struct Command: u8 {
        /// Запись содержимого кэша диска на физический носитель.
        const FLUSH_CACHE = 0xE7;

        /// Чтение диапазона секторов с диска.
        const READ = 0x20;

        /// Получение максимального номера сектора диска.
        const READ_NATIVE_MAX_ADDRESS = 0xF8;

        /// Запись диапазона секторов на диск.
        const WRITE = 0x30;
    }
}

/// Выбирает режим логической адресации блоков диска
/// ([Logical block addressing](https://en.wikipedia.org/wiki/Logical_block_addressing), LBA).
const LBA: u8 = 1 << 6;

/// Зарезервированные биты, которые должны быть выставлены в `1`.
const RESERVED_SHOULD_BE_ONES: u8 = (1 << 5) | (1 << 7);

/// Маска значащих бит в номере сектора.
const SECTOR_NUMBER_MASK: usize = (1 << 28) - 1;

#[doc(hidden)]
pub mod test_scaffolding {
    use ku::error::Result;

    use super::Disk;

    pub fn block_count(disk: usize) -> Result<usize> {
        Disk::new(disk)?.block_count()
    }
}
