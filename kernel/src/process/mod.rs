/// Содержит структуру пользовательского процесса [`Process`].
#[allow(clippy::module_inception)]
mod process;

/// Описывает состояние регистров процесса [`Registers`] и
/// контекст исполнения содержащий уровень привилегий [`ModeContext`].
mod registers;

/// Планировщик процессов.
/// Реализует простейшее
/// [циклическое исполнение процессов](https://en.wikipedia.org/wiki/Round-robin_scheduling).
mod scheduler;

/// Реализует системные вызовы.
pub(crate) mod syscall;

/// Таблица процессов.
mod table;

use ku::process::elf;

use crate::{
    Subsystems,
    allocator::BigPair,
    error::Result,
    log::info,
    memory::{
        BASE_ADDRESS_SPACE,
        KERNEL_R,
        Size,
        USER_R,
    },
};

use process::TrapContext;
use table::TABLE;

pub use ku::process::Pid;

pub use process::Process;
pub use scheduler::Scheduler;
pub use table::Table;

pub(crate) use registers::{
    ModeContext,
    Registers,
};

/// Инициализация подсистемы процессов.
pub fn init(subsystems: Subsystems) {
    if subsystems.contains(Subsystems::SYSCALL) {
        syscall::init();
    }

    if subsystems.contains(Subsystems::PROCESS_TABLE) {
        *TABLE.lock() = Table::new(PROCESS_SLOT_COUNT);
    }

    if subsystems.contains(Subsystems::SCHEDULER) {
        Scheduler::init(PROCESS_SLOT_COUNT);
    }
}

/// Создаёт процесс для заданного
/// [ELF--файла](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)
/// `elf_file`,
/// вставляет его в таблицу процессов и
/// возвращает его идентификатор.
pub fn create(elf_file: &[u8]) -> Result<Pid> {
    Table::allocate(create_process(elf_file)?)
}

/// Создаёт процесс для заданного
/// [ELF--файла](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)
/// `elf_file` и возвращает его.
fn create_process(elf_file: &[u8]) -> Result<Process> {
    let mut base_address_space = BASE_ADDRESS_SPACE.lock();
    let mut process_address_space = base_address_space.duplicate()?;
    let mut src_dst = BigPair::new_pair(
        &mut base_address_space,
        KERNEL_R,
        &mut process_address_space,
        USER_R,
    );

    let entry = unsafe { elf::load(&mut src_dst, elf_file)? };

    drop(base_address_space);

    let process = Process::new(process_address_space, entry)?;

    info!(%entry, file_size = %Size::from_slice(elf_file), %process, "loaded ELF file");

    Ok(process)
}

/// Максимальное количество одновременно работающих процессов.
const PROCESS_SLOT_COUNT: usize = 1 << 8;

#[doc(hidden)]
pub mod test_scaffolding {
    pub use super::{
        process::test_scaffolding::*,
        scheduler::test_scaffolding::*,
        syscall::test_scaffolding::*,
        table::test_scaffolding::*,
    };

    use ku::process::Pid;

    use crate::{
        error::Result,
        memory::{
            BASE_ADDRESS_SPACE,
            Virt,
        },
        process::{
            Process,
            Table,
        },
    };

    pub fn create_process(elf_file: &[u8]) -> Result<Process> {
        super::create_process(elf_file)
    }

    pub fn dummy_process() -> Result<Pid> {
        let address_space = BASE_ADDRESS_SPACE.lock().duplicate()?;

        let process = Process::new(address_space, Virt::default())?;

        Table::allocate(process)
    }

    pub const PROCESS_SLOT_COUNT: usize = super::PROCESS_SLOT_COUNT;
}
