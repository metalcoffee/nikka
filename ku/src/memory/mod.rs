/// Определения типов адресов памяти, как виртуальных, так и физических ---
/// [`Addr`], [`Virt`] и [`Phys`].
pub mod addr;

/// Работа с блоками памяти [`Block`].
pub mod block;

/// Определения типов (виртуальных) страниц памяти и (физических) фреймов ---
/// [`Frage`], [`Frame`] и [`Page`].
pub mod frage;

/// Работа с блоком управления памятью процессора ---
/// [Memory Management Unit](https://en.wikipedia.org/wiki/Memory_management_unit).
pub mod mmu;

/// Информация об
/// [исключении доступа к странице](https://en.wikipedia.org/wiki/Page_fault)
/// (Page Fault) --- [`PageFaultInfo`].
pub mod page_fault_info;

/// Определения типов для
/// [портов ввода-вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
/// [архитектуры x86-64](https://wiki.osdev.org/X86-64).
///
/// Описание портов ввода-вывода и работы с ними:
///   - [Part D](http://www.cs.cmu.edu/~ralf/interrupt-list/inter61d.zip) из
///     [Ralf Brown's Interrupt List](http://www.cs.cmu.edu/~ralf/files.html).
///   - [Intel® 7 Series Chipset Family PCH Datasheet](https://web.archive.org/web/20181006150645/https://www.intel.com/content/dam/www/public/us/en/documents/datasheets/7-series-chipset-pch-datasheet.pdf),
///     секция "9.3 I/O Map".
///   - [XT, AT and PS/2 I/O port addresses](https://bochs.sourceforge.io/techspec/PORTS.LST).
///   - [Linux Device Drivers for your Girl Friend](https://sysplay.github.io/books/LinuxDrivers/book/Content/Part01.html).
///   - [Advanced x86: BIOS and System Management Mode Internals Input/Output](https://opensecuritytraining.info/IntroBIOS_files/Day1_04_Advanced%20x86%20-%20BIOS%20and%20SMM%20Internals%20-%20IO.pdf).
pub mod port;

/// Абстракция размера в памяти [`Size`].
pub mod size;

pub use addr::{
    Phys,
    Virt,
};
pub use block::Block;
pub use frage::{
    ElasticFrame,
    ElasticPage,
    Frame,
    L0_SIZE,
    L0Frame,
    L0Page,
    L1_SIZE,
    L1Frame,
    L1Page,
    L2_SIZE,
    L2Frame,
    L2Page,
    Page,
};
pub use mmu::{
    FULL_ACCESS,
    KERNEL_MMIO,
    KERNEL_R,
    KERNEL_RW,
    KERNEL_RX,
    SYSCALL_ALLOWED_FLAGS,
    USER_R,
    USER_RW,
    USER_RX,
};
pub use page_fault_info::PageFaultInfo;
pub use port::{
    IndexDataPair,
    IndexDataPortPair,
    Port,
};
pub use size::{
    GiB,
    KiB,
    MiB,
    Size,
    SizeOf,
    TiB,
};

// Used in docs.
#[allow(unused)]
use self::{
    addr::Addr,
    frage::Frage,
};

use addr::Tag;
