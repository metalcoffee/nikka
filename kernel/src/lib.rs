#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

//! Библиотека ядра.
//! Позволяет подключать ядро как при обычном запуске, так и при запуске интеграционных тестов.

#![cfg_attr(test, no_main)]
#![deny(warnings)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]
#![feature(custom_test_frameworks)]
#![feature(maybe_uninit_fill, maybe_uninit_slice)]
#![feature(slice_ptr_get)]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::test_runner)]
#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]

extern crate alloc;

/// Аллокаторы памяти общего назначения.
pub mod allocator;

/// Перечисление для возможных ошибок [`Error`] и соответствующий [`Result`].
pub mod error;

/// [Файловая система](https://en.wikipedia.org/wiki/File_system).
pub mod fs;

/// Поддержка журналирования макросами библиотеки [`tracing`].
pub mod log;

/// Здесь находится часть работы с памятью, которая происходит только в ядре.
pub mod memory;

/// Здесь находится часть работы с процессами, которая происходит только в ядре.
pub mod process;

/// Поддержка симметричной многопроцессорности
/// ([Symmetric multiprocessing](https://en.wikipedia.org/wiki/Symmetric_multiprocessing), SMP).
pub mod smp;

/// Здесь находится часть работы со временем, которая происходит только в ядре.
pub mod time;

/// Система обработки [прерываний](https://en.wikipedia.org/wiki/Interrupt).
///
/// External Interrupts in the x86 system:
///   - [Part 1. Interrupt controller evolution](https://habr.com/en/post/446312/)
///   - [Part 2. Linux kernel boot options](https://habr.com/en/post/501660/)
///   - [Part 3. Interrupt routing setup in a chipset, with the example of coreboot](https://habr.com/en/post/501912/)
pub mod trap;

use core::{
    any,
    fmt::Write,
    panic::PanicInfo,
};

use bitflags::bitflags;
use bootloader::BootInfo;
use x86::io;

use ku::{
    self,
    SystemInfo,
    backtrace::Backtrace,
    error::Error::NoPage,
};
use text::println;

use log::{
    info,
    warn,
};
use memory::gdt;

// Used in docs.
#[allow(unused)]
use error::Error;

/// Цвет для печати сообщения об успешном завершении теста.
#[allow(dead_code)]
const PASS: text::Color = text::Color::LIGHT_GREEN;

/// Цвет для печати сообщения о провале теста.
#[allow(dead_code)]
const FAIL: text::Color = text::Color::LIGHT_RED;

bitflags! {
    /// Код выхода из qemu при запуске тестов.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct ExitCode: u8 {
        /// Успешное завершение --- все тесты прошли.
        const SUCCESS = 1;

        /// Неуспешное завершение --- есть провалившийся тест.
        const FAILURE = 2;
    }
}

bitflags! {
    /// Разбивка на подсистемы для возможности инициализации только части из них в тестах.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Subsystems: usize {
        /// Память: подсистема физической памяти.
        const PHYS_MEMORY = 1 << 0;

        /// Память: подсистема виртуальной памяти.
        const VIRT_MEMORY = 1 << 1;

        /// Память: основной аллокатор физической памяти.
        const MAIN_FRAME_ALLOCATOR = 1 << 2;

        /// Все части подсистемы памяти.
        const MEMORY =
            Self::PHYS_MEMORY.bits() | Self::VIRT_MEMORY.bits() | Self::MAIN_FRAME_ALLOCATOR.bits();

        /// Процессы: системные вызовы.
        const SYSCALL = 1 << 4;

        /// Процессы: таблица процессов.
        const PROCESS_TABLE = 1 << 5;

        /// Процессы: планировщик.
        const SCHEDULER = 1 << 6;

        /// Все части подсистемы процессов.
        const PROCESS = Self::SYSCALL.bits() | Self::PROCESS_TABLE.bits() | Self::SCHEDULER.bits();

        /// Симметричной многопроцессорности: контроллер прерываний Local APIC.
        const LOCAL_APIC = 1 << 7;

        /// Симметричной многопроцессорности: вектор структур для CPU--локальных данных.
        const CPUS = 1 << 8;

        /// Симметричной многопроцессорности: запуск Application Processors.
        const BOOT_APS = 1 << 9;

        /// Все части подсистемы симметричной многопроцессорности.
        const SMP = Self::LOCAL_APIC.bits() | Self::CPUS.bits() | Self::BOOT_APS.bits();
    }
}

/// Инициализация части подсистем ядра для тестов.
/// Аргумент `boot_info` содержит информацию от [`bootloader`],
/// `subsystems` задаёт набор подсистем, которые нужно инициализировать.
#[cold]
#[inline(never)]
pub fn init_subsystems(
    boot_info: &'static BootInfo,
    subsystems: Subsystems,
) {
    ku::set_system_info(&SYSTEM_INFO);

    smp::preinit();

    text::TEXT.lock().init();
    log::init();
    time::init();

    info!(now = %time::now(), tsc = ?time::timer(), "Nikka booted");

    gdt::init();
    trap::init();

    let phys2virt = if subsystems.intersects(Subsystems::MEMORY) {
        memory::init(boot_info, subsystems)
    } else {
        Err(NoPage)
    };

    if subsystems.intersects(Subsystems::SMP) &&
        let Ok(phys2virt) = phys2virt
    {
        smp::init(phys2virt, subsystems);
    }

    if subsystems.intersects(Subsystems::PROCESS) {
        process::init(subsystems);
    }
}

/// Инициализация всех подсистем ядра.
/// Аргумент `boot_info` содержит информацию от [`bootloader`].
#[cold]
#[inline(never)]
pub fn init(boot_info: &'static BootInfo) {
    init_subsystems(boot_info, Subsystems::all());
}

/// Выхода из qemu при запуске тестов с кодом `exit_code`.
/// Требует указания аргумента `-device isa-debug-exit,iobase=0xF4,iosize=0x04` при запуске qemu.
pub fn exit_qemu(exit_code: ExitCode) -> ! {
    info!(?exit_code, "exit qemu");

    /// [Порт ввода--вывода](https://wiki.osdev.org/Port_IO)
    /// для команды выхода из qemu.
    const EXIT_PORT: u16 = 0xF4;

    unsafe {
        io::outb(EXIT_PORT, exit_code.bits());

        ku::halt()
    }
}

/// Определяет интерфейс запуска теста.
pub trait Testable {
    /// Запускает тест.
    fn run(&self);
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        println!("\n{:-<60}", any::type_name::<T>());
        self();
        println!(color(PASS), "{:-<51} [passed]", any::type_name::<T>());
    }
}

/// Запускает набор тестов `tests`.
pub fn test_runner(tests: &[&dyn Testable]) {
    println!("running {} tests", tests.len());

    for test in tests {
        test.run();
    }

    exit_qemu(ExitCode::SUCCESS);
}

/// Отмечает интеграционный тест как проваленный.
pub fn fail_test(panic_info: &PanicInfo) -> ! {
    println!(color(FAIL), "{}", panic_info);

    if let Ok(backtrace) = Backtrace::current() {
        println!(color(FAIL), "{backtrace:?}");
    }

    println!(color(FAIL), "{:-<51} [failed]", "");

    exit_qemu(ExitCode::FAILURE)
}

/// Отмечает интеграционный тест как прошедший.
pub fn pass_test() -> ! {
    println!(color(PASS), "{:-<51} [passed]", "");

    exit_qemu(ExitCode::SUCCESS)
}

/// Точка входа для запуска интеграционных тестов.
#[cfg(test)]
#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    test_main();
    exit_qemu(ExitCode::FAILURE)
}

/// Обработчик паники для интеграционных тестов.
#[cfg(test)]
#[cold]
#[inline(never)]
#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    ku::sync::start_panicking();
    fail_test(panic_info)
}

/// Страница памяти с общей информацией о системе.
static SYSTEM_INFO: SystemInfo = SystemInfo::new();
