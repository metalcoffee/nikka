/// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
/// для отслеживания какие именно элементы
/// (блоки или [inode](https://en.wikipedia.org/wiki/Inode) для соответствующей битовой карты)
/// [файловой системы](https://en.wikipedia.org/wiki/File_system) заняты, а какие --- свободны.
mod bitmap;

/// [Блочный кэш](https://en.wikipedia.org/wiki/Page_cache)
/// для ускорения работы с диском за счёт кэширования блоков диска в памяти.
mod block_cache;

/// Запись [директории](https://en.wikipedia.org/wiki/Directory_(computing)) с [`Inode`],
/// который содержится в этой директории, и его именем.
mod directory_entry;

/// Интерфейс для работы с [PATA](https://en.wikipedia.org/wiki/Parallel_ATA)--дисками.
mod disk;

/// Интерфейс к файлaм и директориям файловой системы.
mod file;

/// Интерфейс к файловой системе.
mod file_system;

/// Метаинформация об объекте с данными --- [inode](https://en.wikipedia.org/wiki/Inode).
mod inode;

/// Суперблок
/// ([superblock](https://en.wikipedia.org/wiki/Unix_File_System#Design))
/// файловой системы.
mod superblock;

use ku::memory::Page;

pub use block_cache::BlockCache;
pub use directory_entry::MAX_NAME_LEN;
pub use file::File;
pub use file_system::FileSystem;
pub use inode::Kind;

// Used in docs.
#[allow(unused)]
use {
    bitmap::Bitmap,
    inode::Inode,
    superblock::Superblock,
};

/// Размер блока данных файловой системы.
const BLOCK_SIZE: usize = Page::SIZE;

#[doc(hidden)]
pub mod test_scaffolding {
    pub use super::{
        bitmap::test_scaffolding::*,
        block_cache::test_scaffolding::*,
        disk::test_scaffolding::*,
        file_system::test_scaffolding::*,
        inode::test_scaffolding::*,
        superblock::test_scaffolding::*,
    };

    pub const BLOCK_SIZE: usize = super::BLOCK_SIZE;
}
