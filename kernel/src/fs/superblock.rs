use core::{
    mem,
    ops::Range,
};

use static_assertions::const_assert_eq;

use ku::error::{
    Error::Medium,
    Result,
};

use super::{
    BLOCK_SIZE,
    bitmap::Bitmap,
    block_cache::BlockCache,
    inode::Inode,
};

// Used in docs.
#[allow(unused)]
use ku::error::Error;

// ANCHOR: superblock
/// Суперблок
/// ([superblock](https://en.wikipedia.org/wiki/Unix_File_System#Design))
/// [файловой системы](https://en.wikipedia.org/wiki/File_system).
///
/// Содержит метаинформацию, которая нужна для работы с файловой системой в целом.
#[derive(Debug)]
#[repr(C, align(4096))]
pub struct Superblock {
    /// [Сигнатура](https://en.wikipedia.org/wiki/Magic_number_(programming)#Format_indicators)
    /// файловой системы.
    /// Позволяет убедиться, что на диске действительно хранится
    /// инициализированная файловая система нужного формата.
    magic: [u8; Self::MAGIC.len()],

    /// Индикатор [направления байт](https://en.wikipedia.org/wiki/Endianness)
    /// компьютера, создавшего файловую систему.
    /// Проверяется как и [`Superblock::magic`] чтобы не интерпретировать
    /// данные на диске заведомо неправильным образом.
    endian: u64,

    /// Полное количество блоков в файловой системе, включая блоки с метаданными.
    block_count: usize,

    /// Полное количество [`Inode`] в файловой системе, включая зарезервированные.
    inode_count: usize,
}
// ANCHOR_END: superblock

const_assert_eq!(mem::align_of::<Superblock>(), BLOCK_SIZE);
const_assert_eq!(mem::size_of::<Superblock>(), BLOCK_SIZE);

impl Superblock {
    /// Возвращает суперблок файловой системы или ошибку [`Error::Medium`],
    /// если отведённое под суперблок место диска содержит данные,
    /// которые не похожи на корректный суперблок.
    pub(super) fn new() -> Result<&'static mut Self> {
        Self::new_unchecked().validate()
    }

    /// Форматирует часть диска,
    /// отведённую под суперблок для файловой системы размером `block_count` блоков,
    /// его начальным состоянием.
    pub(super) fn format(
        block_count: usize,
        inode_count: usize,
    ) -> Result<&'static Self> {
        unsafe {
            BlockCache::cache()?
                .block(Self::SUPERBLOCK_BLOCK)
                .try_into_mut_slice::<usize>()
                .unwrap()
                .fill(0);
        }

        let superblock = Self::new_unchecked();

        superblock.magic.copy_from_slice(Self::MAGIC.as_bytes());
        superblock.endian = Self::ENDIAN;
        superblock.block_count = block_count;
        superblock.inode_count = inode_count;

        superblock.validate()?;

        Ok(superblock)
    }

    /// Диапазон блоков, в которых записана
    /// [битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
    /// для отслеживания какие именно блоки файловой системы заняты, а какие --- свободны.
    pub(super) fn block_bitmap(&self) -> Range<usize> {
        let start = Self::FIRST_BITMAP_BLOCK;

        start .. start + Bitmap::size_in_blocks(self.block_count)
    }

    /// Диапазон блоков, в которых записана
    /// [битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
    /// для отслеживания какие именно
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// файловой системы заняты, а какие --- свободны.
    pub(super) fn inode_bitmap(&self) -> Range<usize> {
        let start = self.block_bitmap().end;

        start .. start + Bitmap::size_in_blocks(self.inode_count)
    }

    /// Диапазон блоков, в которых записан массив
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// файловой системы.
    pub(super) fn inode_table(&self) -> Range<usize> {
        let start = self.inode_bitmap().end;

        start .. start + (mem::size_of::<Inode>() * self.inode_count).div_ceil(BLOCK_SIZE)
    }

    /// Возвращает диапазон блоков для пользовательских данных и директорий.
    ///
    /// Полное количество блоков в [файловой системе](https://en.wikipedia.org/wiki/File_system)
    /// равно `Superblock::blocks().end`.
    /// Блоки с нулевого до начала возвращаемого диапазона (не включительно),
    /// то есть диапазон `0..Superblock::blocks().start`,
    /// зарезервированы под метаданные самой файловой системы.
    pub(super) fn blocks(&self) -> Range<usize> {
        let start = self.inode_table().end;

        start .. self.block_count
    }

    /// Возвращает диапазон номеров
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// для пользовательских файлов и директорий.
    ///
    /// Полное количество
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// в [файловой системе](https://en.wikipedia.org/wiki/File_system)
    /// равно `Superblock::inode().end`.
    /// [Inode](https://en.wikipedia.org/wiki/Inode)
    /// с нулевого до начала возвращаемого диапазона (не включительно),
    /// то есть диапазон `0..Superblock::inode().start`,
    /// зарезервированы под
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// самой файловой системы, например под корневую директорию.
    pub(super) fn inodes(&self) -> Range<usize> {
        Self::ROOT_INODE + 1 .. self.inode_count
    }

    /// Номер
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// [корневой директории](https://en.wikipedia.org/wiki/Root_directory)
    /// файловой системы.
    pub(super) fn root(&self) -> usize {
        Self::ROOT_INODE
    }

    /// Создаёт [`Superblock`], не проверяя корректность его данных на диске.
    fn new_unchecked() -> &'static mut Self {
        unsafe {
            BlockCache::cache()
                .unwrap()
                .block(Self::SUPERBLOCK_BLOCK)
                .try_into_mut::<Self>()
                .unwrap()
        }
    }

    /// Проверяет корректность данных [`Superblock`] на диске.
    /// Возвращает ошибку [`Error::Medium`],
    /// если данные на диске заведомо не содержат корректный [`Superblock`],
    /// или сам [`Superblock`] иначе.
    fn validate(&mut self) -> Result<&mut Self> {
        let is_valid = self.magic == Self::MAGIC.as_bytes() &&
            self.endian == Self::ENDIAN &&
            self.blocks().start < self.block_count &&
            self.inodes().start < self.inode_count;

        if is_valid {
            Ok(self)
        } else {
            Err(Medium)
        }
    }

    /// Номер блока, в котором хранится суперблок
    /// ([superblock](https://en.wikipedia.org/wiki/Unix_File_System#Design))
    /// файловой системы --- [`Superblock`].
    pub(super) const SUPERBLOCK_BLOCK: usize = 1;

    /// Значение индикатора [направления байт](https://en.wikipedia.org/wiki/Endianness).
    const ENDIAN: u64 = 0x0102_0304_0506_0708;

    /// Номер первого блока, хранящего
    /// [битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
    /// занятых блоков --- [`Bitmap`].
    const FIRST_BITMAP_BLOCK: usize = Self::SUPERBLOCK_BLOCK + 1;

    /// [Сигнатура](https://en.wikipedia.org/wiki/Magic_number_(programming)#Format_indicators)
    /// файловой системы.
    const MAGIC: &'static str = "Nikka's simple file system";

    /// Номер
    /// [inode](https://en.wikipedia.org/wiki/Inode)
    /// [корневой директории](https://en.wikipedia.org/wiki/Root_directory)
    /// файловой системы.
    const ROOT_INODE: usize = 2;
}

#[doc(hidden)]
pub mod test_scaffolding {
    use core::ops::Range;

    use ku::error::Result;

    pub struct Superblock(&'static super::Superblock);

    impl Superblock {
        pub fn format(
            block_count: usize,
            inode_count: usize,
        ) -> Result<Self> {
            Ok(Superblock(super::Superblock::format(
                block_count,
                inode_count,
            )?))
        }

        pub fn block_bitmap(&self) -> Range<usize> {
            self.0.block_bitmap()
        }

        pub fn inode_bitmap(&self) -> Range<usize> {
            self.0.inode_bitmap()
        }

        pub fn blocks(&self) -> Range<usize> {
            self.0.blocks()
        }
    }
}
