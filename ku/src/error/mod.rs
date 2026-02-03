use core::{
    alloc::LayoutError,
    fmt,
    num::TryFromIntError,
    result,
};

use super::pipe;

/// Перечисление для возможных ошибок.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Ошибка загрузки [ELF--файла](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format).
    Elf(&'static str),

    /// Файл уже существует.
    FileExists,

    /// Файла не существует.
    FileNotFound,

    /// Ошибка форматирования сообщения.
    Fmt(fmt::Error),

    /// Заданное целое значение не помещается в указанный тип.
    Int(TryFromIntError),

    /// Неверное выравнивание.
    InvalidAlignment,

    /// Задано недопустимое значение аргумента.
    InvalidArgument,

    /// Ошибка на устройстве хранения данных.
    Medium,

    /// На данный момент данных нет.
    NoData,

    /// Нет такого устройства хранения данных.
    NoDisk,

    /// Нет свободного физического фрейма памяти.
    NoFrame,

    /// Нет заданной страницы виртуальной памяти.
    NoPage,

    /// Нет заданного процесса.
    NoProcess,

    /// Нет свободного слота в таблице процессов.
    NoProcessSlot,

    /// Заданный путь содержит объект, не являющийся директорией.
    NotDirectory,

    /// Заданный путь указывает на объект, не являющийся файлом.
    NotFile,

    /// Попытка разыменовать нулевой указатель.
    Null,

    /// Возникло переполнение.
    Overflow,

    /// Нарушение прав доступа.
    PermissionDenied,

    /// Ошибка кольцевого буфера --- [`pipe::Error`].
    Pipe(pipe::Error),

    /// Ошибка библиотеки [`postcard`] --- [`postcard::Error`].
    Postcard(postcard::Error),

    /// Истёк тайм-аут.
    Timeout,

    /// Запрошенная функциональность не реализована.
    Unimplemented,
}

impl From<LayoutError> for Error {
    fn from(_e: LayoutError) -> Self {
        Error::InvalidAlignment
    }
}

impl From<fmt::Error> for Error {
    fn from(e: fmt::Error) -> Self {
        Error::Fmt(e)
    }
}

impl From<TryFromIntError> for Error {
    fn from(e: TryFromIntError) -> Self {
        Error::Int(e)
    }
}

impl From<pipe::Error> for Error {
    fn from(e: pipe::Error) -> Self {
        Error::Pipe(e)
    }
}

impl From<postcard::Error> for Error {
    fn from(e: postcard::Error) -> Self {
        Error::Postcard(e)
    }
}

/// Тип возвращаемого результата `T` или ошибки [`Error`] ---
/// мономорфизация [`result::Result`] по типу ошибки.
pub type Result<T> = result::Result<T, Error>;
