use num_enum::{
    IntoPrimitive,
    TryFromPrimitive,
};

use crate::error::{
    Error,
    Result,
};

/// Код выхода пользовательской программы, передаваемый в `syscall::exit()`.
#[derive(Clone, Copy, Debug, Eq, IntoPrimitive, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum ExitCode {
    /// Пользовательская программа завершилась успешно.
    Ok = 0,

    /// Пользовательская программа запаниковала.
    Panic = 1,

    /// Пользовательская программа выполнила несуществующий системный вызов.
    UnimplementedSyscall = 2,
}

/// Номера системных вызовов.
#[derive(Clone, Copy, Debug, Eq, IntoPrimitive, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum Syscall {
    /// Номер системного вызова `exit()`.
    Exit = 0,

    /// Номер системного вызова `log_value()`.
    LogValue = 1,

    /// Номер системного вызова `sched_yield()`.
    SchedYield = 2,

    /// Номер системного вызова `sched_exofork()`.
    Exofork = 3,

    /// Номер системного вызова `map()`.
    Map = 4,

    /// Номер системного вызова `unmap()`.
    Unmap = 5,

    /// Номер системного вызова `copy_mapping()`.
    CopyMapping = 6,

    /// Номер системного вызова `set_state()`.
    SetState = 7,

    /// Номер системного вызова `set_trap_handler()`.
    SetTrapHandler = 8,
}

/// Код ошибки, возвращаемый из системных вызовов.
#[derive(Clone, Copy, Debug, Eq, IntoPrimitive, PartialEq, TryFromPrimitive)]
#[repr(usize)]
pub enum ResultCode {
    /// Код для [`Result::Ok`].
    Ok = 0,

    /// Код для всех ошибок, которые не должны возвращаться из системных вызовов.
    Unexpected = 1,

    /// Код для [`Error::InvalidArgument`].
    InvalidArgument = 2,

    /// Код для [`Error::NoFrame`].
    NoFrame = 3,

    /// Код для [`Error::NoPage`].
    NoPage = 4,

    /// Код для [`Error::NoProcess`].
    NoProcess = 5,

    /// Код для [`Error::NoProcessSlot`].
    NoProcessSlot = 6,

    /// Код для [`Error::Null`].
    Null = 7,

    /// Код для [`Error::Overflow`].
    Overflow = 8,

    /// Код для [`Error::PermissionDenied`].
    PermissionDenied = 9,

    /// Код для [`Error::Unimplemented`].
    Unimplemented = 10,

    /// Код для [`Error::InvalidAlignment`].
    InvalidAlignment = 11,
}

impl From<ResultCode> for Result<()> {
    fn from(result: ResultCode) -> Result<()> {
        match result {
            ResultCode::Ok => Ok(()),

            ResultCode::InvalidArgument => Err(Error::InvalidArgument),
            ResultCode::NoFrame => Err(Error::NoFrame),
            ResultCode::NoPage => Err(Error::NoPage),
            ResultCode::NoProcess => Err(Error::NoProcess),
            ResultCode::NoProcessSlot => Err(Error::NoProcessSlot),
            ResultCode::Null => Err(Error::Null),
            ResultCode::Overflow => Err(Error::Overflow),
            ResultCode::PermissionDenied => Err(Error::PermissionDenied),
            ResultCode::Unimplemented => Err(Error::Unimplemented),
            ResultCode::InvalidAlignment => Err(Error::InvalidAlignment),

            _ => panic!("unexpected error {:?}", result),
        }
    }
}

impl<T> From<Result<T>> for ResultCode {
    fn from(result: Result<T>) -> ResultCode {
        match result {
            Ok(_) => ResultCode::Ok,

            Err(error) => match error {
                Error::Elf(_) => ResultCode::Unexpected,
                Error::Fmt(_) => ResultCode::Unexpected,
                Error::Int(_) => ResultCode::Unexpected,
                Error::InvalidArgument => ResultCode::InvalidArgument,
                Error::NoFrame => ResultCode::NoFrame,
                Error::NoPage => ResultCode::NoPage,
                Error::NoProcess => ResultCode::NoProcess,
                Error::NoProcessSlot => ResultCode::NoProcessSlot,
                Error::Null => ResultCode::Null,
                Error::Overflow => ResultCode::Overflow,
                Error::PermissionDenied => ResultCode::PermissionDenied,
                Error::Pipe(_) => ResultCode::Unexpected,
                Error::Postcard(_) => ResultCode::Unexpected,
                Error::Unimplemented => ResultCode::Unimplemented,
                Error::InvalidAlignment => ResultCode::InvalidAlignment,

                _ => ResultCode::Unexpected,
            },
        }
    }
}
