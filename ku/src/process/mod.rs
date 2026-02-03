/// Загружает [ELF--файл](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)
/// пользовательского процесса в его адресное пространство.
pub mod elf;

/// Минимальная информация о контексте исполнения.
pub mod mini_context;

/// Идентификатор процесса.
pub mod pid;

/// Операции с [регистром флагов](https://en.wikipedia.org/wiki/FLAGS_register).
pub mod registers;

/// Константы для работы с системными вызовами.
pub mod syscall;

/// Информация об исключении процессора.
pub mod trap_info;

use num_enum::{
    IntoPrimitive,
    TryFromPrimitive,
};

pub use mini_context::MiniContext;
pub use pid::Pid;
pub use registers::RFlags;
pub use syscall::{
    ExitCode,
    ResultCode,
    Syscall,
};
pub use trap_info::{
    Info,
    RSP_OFFSET_IN_TRAP_INFO,
    Trap,
    TrapInfo,
};

/// Состояние пользовательского процесса.
#[derive(Clone, Copy, Debug, Eq, IntoPrimitive, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum State {
    /// Процесс только что создан и не готов к запуску.
    Exofork = 0,

    /// Процесс готов к вытеснению, но не выполняется в данный момент.
    Runnable = 1,

    /// Процесс выполняется в данный момент.
    Running = 2,
}

#[doc(hidden)]
pub mod test_scaffolding {
    pub use super::elf::test_scaffolding::*;
}
