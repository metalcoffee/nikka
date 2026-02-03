use alloc::string::String;

// Used in docs.
#[allow(unused)]
use ku::error::Error;

/// Интерфейс к файлaм и директориям файловой системы.
#[derive(Debug)]
pub struct File {
    /// Номер [inode](https://en.wikipedia.org/wiki/Inode) файла.
    inode: usize,

    /// Имя файла.
    name: String,

    /// [Inode](https://en.wikipedia.org/wiki/Inode)
    /// директории, содержащей файл.
    parent: usize,
}

impl File {
    /// Создаёт [`File`] для доступа к заданному `inode`.
    pub(super) fn new(
        inode: usize,
        name: &str,
        parent: usize,
    ) -> Self {
        Self {
            inode,
            name: name.into(),
            parent,
        }
    }

    /// Номер [inode](https://en.wikipedia.org/wiki/Inode) файла.
    pub(super) fn inode(&self) -> usize {
        self.inode
    }

    /// Имя файла.
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// [Inode](https://en.wikipedia.org/wiki/Inode)
    /// директории, содержащей файл.
    pub(super) fn parent(&self) -> usize {
        self.parent
    }
}
