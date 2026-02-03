use alloc::{
    string::String,
    vec::Vec,
};
use core::fmt;

use chrono::{
    DateTime,
    Utc,
};

use ku::{
    collections::Lru,
    error::{
        Error::{
            FileNotFound,
            Medium,
            NotDirectory,
        },
        Result,
    },
    log::{
        debug,
        error,
        info,
    },
    memory::size::Size,
};

use super::{
    BLOCK_SIZE,
    bitmap::Bitmap,
    block_cache::BlockCache,
    directory_entry::DirectoryEntry,
    disk::Disk,
    file::File,
    inode::{
        Inode,
        Kind,
    },
    superblock::Superblock,
};

// Used in docs.
#[allow(unused)]
use ku::error::Error;

/// Интерфейс к файловой системе.
#[derive(Debug)]
pub struct FileSystem {
    /// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
    /// для отслеживания какие именно блоки файловой системы заняты, а какие --- свободны.
    block_bitmap: Bitmap,

    /// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
    /// для отслеживания какие именно
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// файловой системы заняты, а какие --- свободны.
    inode_bitmap: Bitmap,

    /// Массив
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// файловой системы.
    inodes: &'static mut [Inode],

    /// Кэш для определения
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// файла по
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// директории и имени файла в ней.
    resolve_cache: Lru<(usize, String), usize>,

    /// Суперблок
    /// ([superblock](https://en.wikipedia.org/wiki/Unix_File_System#Design))
    /// файловой системы.
    superblock: &'static mut Superblock,
}

impl FileSystem {
    /// [Монтирует](https://en.wikipedia.org/wiki/Mount_(computing))
    /// файловую систему с диска номер `disk`.
    /// Параметр `block_cache_capacity` задаёт ограничение на размер кэша блоков.
    /// Параметр `resolve_cache_capacity` задаёт ограничение на размер кэша
    /// для преобразования имён файлов в их номера inode.
    pub fn mount(
        disk: usize,
        block_cache_capacity: usize,
        resolve_cache_capacity: usize,
    ) -> Result<FileSystem> {
        let disk = Disk::new(disk)?;
        let block_count = disk.block_count()?;

        BlockCache::init(disk, block_count, block_cache_capacity)?;

        let superblock = Superblock::new()?;

        let inodes = unsafe {
            BlockCache::cache()?
                .block(superblock.inode_table().start)
                .start_address()
                .try_into_mut_slice::<Inode>(superblock.inodes().end)?
        };

        if superblock.blocks().end <= block_count &&
            inodes[superblock.root()].kind() == Kind::Directory
        {
            Ok(Self {
                block_bitmap: Bitmap::new(superblock.block_bitmap().start, superblock.blocks())?,
                inode_bitmap: Bitmap::new(superblock.inode_bitmap().start, superblock.inodes())?,
                inodes,
                resolve_cache: Lru::new(resolve_cache_capacity),
                superblock,
            })
        } else {
            Err(Medium)
        }
    }

    /// Форматирует файловую систему на диске номер `disk`.
    pub fn format(disk: usize) -> Result<()> {
        let disk = Disk::new(disk)?;
        let block_count = disk.block_count()?;

        let default_blocks_per_inode = 4;
        let block_cache_capacity = 1 << 10;
        let inode_count = block_count / default_blocks_per_inode;

        BlockCache::init(disk, block_count, block_cache_capacity)?;

        let superblock = Superblock::format(block_count, inode_count)?;
        Bitmap::format(superblock.block_bitmap().start, superblock.blocks())?;
        Bitmap::format(superblock.inode_bitmap().start, superblock.inodes())?;

        let inodes = unsafe {
            BlockCache::cache()?
                .block(superblock.inode_table().start)
                .start_address()
                .try_into_mut_slice::<Inode>(superblock.inodes().end)?
        };
        for inode in inodes.iter_mut() {
            *inode = Inode::default();
        }
        inodes[superblock.root()].init(Kind::Directory);

        let block_size = Size::bytes(BLOCK_SIZE);
        let blocks = superblock.blocks();
        let inodes = superblock.inodes();
        let inode_size = Size::of::<Inode>();
        let directory_entry_size = Size::of::<DirectoryEntry>();
        let free_space = Size::bytes(blocks.clone().count() * BLOCK_SIZE);
        let max_file_size = Size::bytes(Inode::max_size());
        info!(
            %free_space,
            %disk,
            %block_size,
            block_count,
            ?blocks,
            %inode_size,
            ?inodes,
            %directory_entry_size,
            %max_file_size,
            "formatted the file system",
        );

        BlockCache::flush(superblock.blocks().start)
    }

    /// Записывает на диск все обновления файловой системы.
    pub fn flush(&self) -> Result<()> {
        BlockCache::flush(self.superblock.blocks().end)
    }

    // ANCHOR: open
    /// Проходит от корня файловой системы по заданному полному пути `path`.
    /// Возвращает [`File`], соответствующий этому `path`.
    pub fn open(
        &mut self,
        path: &str,
    ) -> Result<File> {
        // ANCHOR_END: open
        // TODO: your code here.
        unimplemented!();
    }

    /// Тип --- файл или директория.
    pub fn kind(
        &self,
        file: &File,
    ) -> Kind {
        self.inodes[file.inode()].kind()
    }

    /// Время последней модификации файла или директории.
    pub fn modify_time(
        &self,
        file: &File,
    ) -> DateTime<Utc> {
        self.inodes[file.inode()].modify_time()
    }

    /// Размер данных в байтах.
    pub fn size(
        &self,
        file: &File,
    ) -> usize {
        self.inodes[file.inode()].size()
    }

    /// Устанавливает размер данных в байтах.
    /// Если файл расширяется, то новые блоки с данными содержат нули.
    /// При необходимости выделяет или освобождает блоки.
    /// Обновляет время последней модификации файла.
    pub fn set_size(
        &mut self,
        file: &File,
        size: usize,
    ) -> Result<()> {
        self.inodes[file.inode()].set_size(size, &mut self.block_bitmap)
    }

    /// Находит файл или поддиректорию с именем `name` в директории `directory`.
    /// Возвращает ошибку [`Error::FileNotFound`], если такого файла нет.
    pub fn find(
        &mut self,
        directory: &File,
        name: &str,
    ) -> Result<File> {
        self.inodes[directory.inode()]
            .find(name)
            .map(|directory_entry| File::new(directory_entry.inode(), name, directory.inode()))
    }

    /// Возвращает список файлов и поддиректорий в директории.
    pub fn list(
        &mut self,
        directory: &File,
    ) -> Result<Vec<Entry>> {
        let mut list = Vec::new();

        for directory_entry in self.inodes[directory.inode()].list()? {
            let entry = Entry {
                inode: directory_entry.inode(),
                kind: Kind::default(),
                modify_time: DateTime::default(),
                name: String::from(directory_entry.name()?),
                size: 0,
            };
            list.push(entry);
        }

        for entry in list.iter_mut() {
            let inode = &self.inodes[entry.inode];
            entry.kind = inode.kind();
            entry.modify_time = inode.modify_time();
            entry.size = inode.size();
        }

        Ok(list)
    }

    /// Вставляет в директорию запись с именем `name` и типом `kind`.
    /// Обновляет как время модификации выделенной записи, так и время модификации самой директории.
    ///
    /// Возвращает ошибку [`Error::FileExists`] если запись с таким именем уже есть.
    pub fn insert(
        &mut self,
        directory: &File,
        name: &str,
        kind: Kind,
    ) -> Result<File> {
        let entry = self.inodes[directory.inode()].insert(name, &mut self.block_bitmap)?;
        let inode = self.inode_bitmap.allocate()?;
        entry.set_inode(inode);
        self.inodes[inode].init(kind);

        Ok(File::new(inode, name, directory.inode()))
    }

    /// Удаляет файл.
    pub fn remove(
        &mut self,
        file: &File,
    ) -> Result<()> {
        self.resolve_cache.remove(&(file.parent(), file.name().into()));
        self.inodes[file.parent()].find(file.name())?.set_free();
        self.inode_bitmap.set_free(file.inode());
        self.remove_inode(file.inode())
    }

    /// Читает из файла по смещению `offset` в буфер `buffer` столько байт,
    /// сколько остаётся до конца файла или до конца буфера.
    ///
    /// Возвращает количество прочитанных байт.
    /// Если `offset` равен размеру файла, возвращает `0` прочитанных байт.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::NotFile`] если [Inode](https://en.wikipedia.org/wiki/Inode) не является файлом.
    ///   - [`Error::InvalidArgument`] если `offset` превышает размер файла.
    pub fn read(
        &mut self,
        file: &File,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<usize> {
        self.inodes[file.inode()].read(offset, buffer)
    }

    /// Записывает в файл по смещению `offset` байты из буфера `buffer`.
    /// При необходимости расширяет размер файла.
    ///
    /// Возвращает количество записанных байт.
    /// Если `offset` превышает размер файла, расширяет файл нулями до заданного `offset`.
    ///
    /// Возвращает ошибку [`Error::NotFile`]
    /// если [Inode](https://en.wikipedia.org/wiki/Inode) не является файлом.
    pub fn write(
        &mut self,
        file: &File,
        offset: usize,
        buffer: &[u8],
    ) -> Result<usize> {
        self.inodes[file.inode()].write(offset, buffer, &mut self.block_bitmap)
    }

    /// Возвращает размер свободного места файловой системы в байтах.
    pub fn free_space(&self) -> usize {
        self.block_bitmap.free_count() * BLOCK_SIZE
    }

    /// Возвращает размер занятого места файловой системы в байтах.
    pub fn used_space(&self) -> usize {
        self.superblock.blocks().count() * BLOCK_SIZE - self.free_space()
    }

    /// Удаляет `inode`.
    pub fn remove_inode(
        &mut self,
        inode: usize,
    ) -> Result<()> {
        self.inodes[inode].remove(&mut self.block_bitmap)
    }
}

impl Drop for FileSystem {
    fn drop(&mut self) {
        if let Err(error) = self.flush() {
            error!(?error, "error on the file system unmount");
        }

        debug!(
            block_cache_stats = ?BlockCache::stats(),
            resolve_cache_stats = ?self.resolve_cache.stats(),
            "unmount",
        );
    }
}

/// Элемент списка файлов и поддиректорий в директории.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Номер [inode](https://en.wikipedia.org/wiki/Inode) файла.
    inode: usize,

    /// Тип --- файл или поддиректория.
    kind: Kind,

    /// Время последней модификации файла или поддиректории.
    modify_time: DateTime<Utc>,

    /// Имя файла или поддиректории.
    name: String,

    /// Размер файла или поддиректории в байтах.
    size: usize,
}

impl Entry {
    /// Тип --- файл или поддиректория.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Время последней модификации файла или поддиректории.
    pub fn modify_time(&self) -> DateTime<Utc> {
        self.modify_time
    }

    /// Имя файла или поддиректории.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Размер файла или поддиректории в байтах.
    pub fn size(&self) -> usize {
        self.size
    }
}

impl fmt::Display for Entry {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{}, {}, {:?}, {} B = {}, {}",
            self.inode,
            self.name(),
            self.kind(),
            self.size(),
            Size::bytes(self.size()),
            self.modify_time(),
        )
    }
}

#[doc(hidden)]
pub mod test_scaffolding {
    use ku::error::Result;

    use super::{
        File,
        FileSystem,
        Kind,
    };

    pub fn make_file(
        file_system: &mut FileSystem,
        kind: Kind,
    ) -> File {
        file_system.inodes[file_system.superblock.root()].init(kind);
        File::new(file_system.superblock.root(), "", 0)
    }

    pub fn remove_file(
        file_system: &mut FileSystem,
        file: &File,
    ) -> Result<()> {
        file_system.remove_inode(file.inode())
    }
}
