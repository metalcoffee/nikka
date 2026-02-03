use core::{
    fmt,
    mem,
    str,
};

use static_assertions::const_assert_eq;

use ku::error::{
    Error::{
        InvalidArgument,
        Medium,
    },
    Result,
};

use super::BLOCK_SIZE;

// Used in docs.
#[allow(unused)]
use {
    super::inode::Inode,
    ku::error::Error,
};

// ANCHOR: directory_entry
/// Запись [директории](https://en.wikipedia.org/wiki/Directory_(computing)) с [`Inode`],
/// который содержится в этой директории, и его именем.
#[derive(Debug)]
#[repr(C)]
pub(super) struct DirectoryEntry {
    /// [Inode](https://en.wikipedia.org/wiki/Inode) записи [`DirectoryEntry`].
    inode: usize,

    /// Имя соответствующего записи файла или поддиректории.
    name: [u8; MAX_NAME_LEN],
}
// ANCHOR_END: directory_entry

impl DirectoryEntry {
    /// Возвращает `true`, если запись директории свободна.
    pub(super) fn is_free(&self) -> bool {
        self.inode() == Self::UNUSED
    }

    /// Освобождает запись директории.
    pub(super) fn set_free(&mut self) {
        self.set_inode(Self::UNUSED)
    }

    /// Возвращает
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// записи [`DirectoryEntry`].
    pub(super) fn inode(&self) -> usize {
        self.inode
    }

    /// Устанавливает
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// записи [`DirectoryEntry`].
    pub(super) fn set_inode(
        &mut self,
        inode: usize,
    ) {
        self.inode = inode
    }

    /// Возвращает имя файла или поддиректории.
    ///
    /// Возвращает ошибку [`Error::Medium`],
    /// если имя на диске не корректно.
    pub(super) fn name(&self) -> Result<&str> {
        let len = self
            .name
            .iter()
            .enumerate()
            .find(|x| *x.1 == 0)
            .map(|x| x.0)
            .unwrap_or(MAX_NAME_LEN);

        let name = str::from_utf8(&self.name[.. len]).map_err(|_| Medium)?;

        Self::validate(name).map_err(|_| Medium)?;

        Ok(name)
    }

    /// Устанавливает имя `name` для файла или поддиректории.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`],
    /// если `name` содержит не-[ASCII](https://en.wikipedia.org/wiki/ASCII) символы
    /// или разделитель пути `/`.
    pub(super) fn set_name(
        &mut self,
        name: &str,
    ) -> Result<()> {
        Self::validate(name)?;

        let bytes = name.as_bytes();
        self.name[.. bytes.len()].copy_from_slice(bytes);
        self.name[bytes.len() ..].fill(0);

        Ok(())
    }

    /// Проверяет допустимость имени файла или директории.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`],
    /// если `name` содержит не-[ASCII](https://en.wikipedia.org/wiki/ASCII) символы
    /// или разделитель пути `/`.
    fn validate(name: &str) -> Result<()> {
        let bytes = name.as_bytes();

        if bytes.is_empty() ||
            bytes.len() > MAX_NAME_LEN ||
            bytes.iter().any(|&x| x == 0 || x == b'/' || !x.is_ascii())
        {
            Err(InvalidArgument)
        } else {
            Ok(())
        }
    }

    /// Зарезервированный номер
    /// [inode](https://en.wikipedia.org/wiki/Inode),
    /// означающий что запись директории [`DirectoryEntry`] свободна.
    const UNUSED: usize = 0;
}

impl fmt::Display for DirectoryEntry {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{:?} {}", self.name(), self.inode())
    }
}

const_assert_eq!(BLOCK_SIZE % mem::size_of::<DirectoryEntry>(), 0);

// ANCHOR: max_name_len
/// Максимальный размер имён файлов и директорий.
pub const MAX_NAME_LEN: usize = (1 << 7) - mem::size_of::<usize>();
// ANCHOR_END: max_name_len
