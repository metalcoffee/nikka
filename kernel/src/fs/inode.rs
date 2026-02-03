use core::{
    cmp::{
        self,
        Ordering,
    },
    fmt,
    iter::Filter,
    mem::{
        self,
        MaybeUninit,
    },
    ops::Add,
};

use chrono::{
    DateTime,
    Utc,
};
use static_assertions::const_assert_eq;

use ku::{
    error::{
        Error::{
            FileExists,
            FileNotFound,
            InvalidArgument,
            NoDisk,
            NotDirectory,
            NotFile,
        },
        Result,
    },
    memory::{
        Block,
        Virt,
        size::Size,
    },
    time,
};

use super::{
    BLOCK_SIZE,
    bitmap::Bitmap,
    block_cache::{
        BlockCache,
        Cache,
    },
    directory_entry::DirectoryEntry,
};

// Used in docs.
#[allow(unused)]
use ku::error::Error;

// ANCHOR: kind
/// Тип объекта с данными --- [inode](https://en.wikipedia.org/wiki/Inode).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(usize)]
pub enum Kind {
    /// Файл.
    #[default]
    File = 0,

    /// Директория.
    Directory = 1,
}
// ANCHOR_END: kind

// ANCHOR: inode
/// Метаинформация об объекте с данными --- [inode](https://en.wikipedia.org/wiki/Inode).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(super) struct Inode {
    /// Тип объекта с данными --- файл или директория.
    kind: Kind,

    /// Время последней модификации [`Inode`].
    modify_time: DateTime<Utc>,

    /// Размер данных в байтах.
    size: usize,

    /// Лес, отвечающий за отображение блоков [`Inode`]
    /// в номера блоков файловой системы.
    root_blocks: Forest,
}
// ANCHOR_END: inode

impl Inode {
    /// Инициализирует [`Inode`] заданным типом `kind`.
    pub(super) fn init(
        &mut self,
        kind: Kind,
    ) {
        self.kind = kind;
        self.modify_time = time::now_ms();
        self.size = 0;
        self.root_blocks.fill(NO_BLOCK);
    }

    /// Тип объекта с данными --- файл или директория.
    pub(super) fn kind(&self) -> Kind {
        self.kind
    }

    /// Удаляет [`Inode`].
    pub(super) fn remove(
        &mut self,
        block_bitmap: &mut Bitmap,
    ) -> Result<()> {
        self.set_size(0, block_bitmap)?;
        assert!(self.root_blocks.iter().all(|&block| block == NO_BLOCK));

        Ok(())
    }

    /// Размер данных в байтах.
    pub(super) fn size(&self) -> usize {
        self.size
    }

    // ANCHOR: set_size
    /// Устанавливает размер данных в байтах.
    ///
    /// Если файл расширяется, то новые блоки с данными содержат нули.
    /// При необходимости выделяет или освобождает блоки через `block_bitmap`.
    /// Обновляет время последней модификации [`Inode`].
    ///
    /// Если новый размер `size` равен нулю, должен освободить все косвенные блоки,
    /// используемые в [`Inode::root_blocks`].
    pub(super) fn set_size(
        &mut self,
        size: usize,
        block_bitmap: &mut Bitmap,
    ) -> Result<()> {
        // ANCHOR_END: set_size
        // TODO: your code here.
        unimplemented!();
    }

    /// Возвращает максимальный размер в байтах, которые может иметь файл.
    pub(super) fn max_size() -> usize {
        let max_blocks = leaf_count();

        max_blocks * BLOCK_SIZE
    }

    /// Время последней модификации [`Inode`].
    pub(super) fn modify_time(&self) -> DateTime<Utc> {
        self.modify_time
    }

    /// Находит занятую запись с именем `name` в директории.
    ///
    /// Возвращает ошибку [`Error::FileNotFound`], если такой записи нет.
    pub(super) fn find(
        &mut self,
        name: &str,
    ) -> Result<&mut DirectoryEntry> {
        self.find_entry(name, None).and_then(|entry| {
            if entry.is_free() {
                Err(FileNotFound)
            } else {
                Ok(entry)
            }
        })
    }

    // ANCHOR: insert
    /// Вставляет в директорию запись с именем `name` и возвращает ссылку на неё.
    /// Обновляет время модификации директории.
    ///
    /// Возвращает ошибку [`Error::FileExists`] если запись с таким именем уже есть.
    pub(super) fn insert(
        &mut self,
        name: &str,
        block_bitmap: &mut Bitmap,
    ) -> Result<&mut DirectoryEntry> {
        // ANCHOR_END: insert
        // TODO: your code here.
        unimplemented!();
    }

    /// Возвращает итератор по занятым записям директории.
    pub(super) fn list(&mut self) -> Result<List<'_>> {
        Ok(List(self.iter()?.filter(|entry| !entry.is_free())))
    }

    // ANCHOR: read
    /// Читает из файла по смещению `offset` в буфер `buffer` столько байт,
    /// сколько остаётся до конца файла или до конца буфера.
    ///
    /// Возвращает количество прочитанных байт.
    /// Если `offset` равен размеру файла, возвращает `0` прочитанных байт.
    ///
    /// Возвращает ошибки:
    ///
    /// - [`Error::NotFile`] если [`Inode`] не является файлом.
    /// - [`Error::InvalidArgument`] если `offset` превышает размер файла.
    pub(super) fn read(
        &mut self,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<usize> {
        // ANCHOR_END: read
        // TODO: your code here.
        unimplemented!();
    }

    // ANCHOR: write
    /// Записывает в файл по смещению `offset` байты из буфера `buffer`.
    /// При необходимости расширяет размер файла.
    /// Обновляет время модификации файла.
    ///
    /// Возвращает количество записанных байт.
    /// Если `offset` превышает размер файла, расширяет файл нулями до заданного `offset`.
    ///
    /// Возвращает ошибку [`Error::NotFile`] если [`Inode`] не является файлом.
    pub(super) fn write(
        &mut self,
        offset: usize,
        buffer: &[u8],
        block_bitmap: &mut Bitmap,
    ) -> Result<usize> {
        // ANCHOR_END: write
        // TODO: your code here.
        unimplemented!();
    }

    // ANCHOR: find_entry
    /// Находит занятую запись с именем `name` в директории.
    ///
    /// Если такой записи нет, возвращает свободную запись --- [`DirectoryEntry::is_free()`].
    /// Если и таких нет, и при этом `block_bitmap` является [`Some`],
    /// пробует расширить директорию и вернуть новую свободную запись.
    /// Иначе возвращает ошибку [`Error::FileNotFound`].
    fn find_entry(
        &mut self,
        name: &str,
        block_bitmap: Option<&mut Bitmap>,
    ) -> Result<&mut DirectoryEntry> {
        // ANCHOR_END: find_entry
        // TODO: your code here.
        unimplemented!();
    }

    /// Возвращает итератор по всем записям директории, --- и занятым, и свободным.
    fn iter(&mut self) -> Result<Iter<'_>> {
        if self.kind == Kind::Directory {
            assert_eq!(self.size % BLOCK_SIZE, 0);

            Ok(Iter {
                block: Block::default(),
                block_number: 0,
                cache: BlockCache::cache()?,
                entry: 0,
                inode: self,
            })
        } else {
            Err(NotDirectory)
        }
    }

    // ANCHOR: block_entry
    /// По номеру блока `inode_block_number` внутри данных [`Inode`]
    /// возвращает ссылку на запись из метаданных этого [`Inode`].
    ///
    /// Эта запись предназначена для номера блока на диске,
    /// хранящего указанный блок данных [`Inode`].
    /// С помощью этого метода происходит отображение смещения внутри данных файла
    /// в номер блока на диске, хранящего эти данные.
    /// Номер `inode_block_number` равен смещению внутри данных файла,
    /// делённому на размер блока [`BLOCK_SIZE`].
    ///
    /// Если при обходе леса отображения блоков `Inode::root_blocks`
    /// встречается не выделенный косвенный блок, пробует выделить его с помощью `block_bitmap`.
    /// Если при этом `block_bitmap` равен [`None`], возвращает ошибку [`Error::NoDisk`].
    fn block_entry(
        &mut self,
        inode_block_number: usize,
        block_bitmap: Option<&mut Bitmap>,
        cache: Cache,
    ) -> Result<&mut usize> {
        // ANCHOR_END: block_entry
        // TODO: your code here.
        unimplemented!();
    }

    // ANCHOR: block
    /// Возвращает блок в памяти блочного кэша,
    /// где хранится блок `inode_block_number` внутри данных [`Inode`].
    fn block(
        &mut self,
        inode_block_number: usize,
        cache: Cache,
    ) -> Result<Block<Virt>> {
        // ANCHOR_END: block
        assert!(inode_block_number < self.size.div_ceil(BLOCK_SIZE));
        Ok(cache.block(*self.block_entry(inode_block_number, None, cache)?))
    }
}

impl fmt::Display for Inode {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ {:?}, size: {} B = {}, modify: {} }}",
            self.kind(),
            self.size(),
            Size::bytes(self.size()),
            self.modify_time(),
        )
    }
}

// ANCHOR: iter
/// Итератор по всем записям директории, --- и занятым, и свободным.
pub(super) struct Iter<'a> {
    /// Блок в памяти блочного кэша, внутри которого находится текущая позиция итератора.
    block: Block<Virt>,

    /// Номер блока внутри данных [`Inode`] для текущей позиции итератора.
    block_number: usize,

    /// Диапазон памяти для кэширования блоков.
    cache: Cache,

    /// Индекс [`DirectoryEntry`] внутри блока для текущей позиции итератора.
    entry: usize,

    /// [`Inode`] самой директории.
    inode: &'a mut Inode,
}
// ANCHOR_END: iter

impl<'a> Iter<'a> {
    /// Расширяет директорию свободными записями на один блок.
    fn extend(
        &mut self,
        block_bitmap: &mut Bitmap,
    ) -> Result<()> {
        let block_number = self.inode.size() / BLOCK_SIZE;

        self.inode.set_size(self.inode.size() + BLOCK_SIZE, block_bitmap)?;

        let block = self.inode.block(block_number, self.cache)?;

        let entries = unsafe {
            block
                .try_into_mut_slice::<MaybeUninit<DirectoryEntry>>()
                .expect(Self::BAD_MEMORY_BLOCK)
                .assume_init_mut()
        };
        for entry in entries.iter_mut() {
            entry.set_free();
        }

        Ok(())
    }

    /// Блок виртуальных адресов не подходит для хранения массива [`DirectoryEntry`].
    /// Возможно, есть проблема с размером самой [`DirectoryEntry`] ---
    /// он должен делить [`BLOCK_SIZE`] нацело.
    const BAD_MEMORY_BLOCK: &'static str = "bad memory block for directory entries";
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a mut DirectoryEntry;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: your code here.
        unimplemented!();
    }
}

/// Итератор по занятым записям директории.
pub(super) struct List<'a>(Filter<Iter<'a>, fn(&&mut DirectoryEntry) -> bool>);

impl<'a> Iterator for List<'a> {
    type Item = &'a mut DirectoryEntry;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

// ANCHOR: forest
/// Лес, отвечающий за отображение блоков [`Inode`]
/// в номера блоков файловой системы.
type Forest = [usize; MAX_HEIGHT];
// ANCHOR_END: forest

// ANCHOR: find_leaf
/// Вспомогательная функция для обхода леса отображения блоков [`Inode`].
/// По заданному количеству `tree_count` деревьев в лесу и номеру блока с данными `leaf_number`
/// выдает кортеж с номером нужного дерева, количеством листьев в этом дереве и номером листа
/// в этом дереве, который соответствует листу `leaf_number` леса.
fn find_leaf(leaf_in_forest: usize) -> Result<LeafCoordinates> {
    // ANCHOR_END: find_leaf
    // TODO: your code here.
    unimplemented!();
}

/// Полное количество листьев в лесу [`Forest`] ---
/// максимальное количество блоков в одном [`Inode`].
fn leaf_count() -> usize {
    (0 .. MAX_HEIGHT)
        .map(leaf_count_in_tree)
        .reduce(Add::add)
        .expect("the forest is empty")
}

/// Количество листьев в дереве высоты `tree_height`.
fn leaf_count_in_tree(tree_height: usize) -> usize {
    INDIRECT_BLOCK_ARITY.pow(tree_height.try_into().expect("the tree height is off the chart"))
}

// ANCHOR: remove_tree
/// Удаляет из леса отображения блоков [`Inode`] дерево или поддерево,
/// на которое указывает запись `node`.
/// Высота поддерева передаётся в `height`.
/// Освобождаемые косвенные блоки передаются в `block_bitmap`.
fn remove_tree(
    node: &mut usize,
    height: usize,
    block_bitmap: &mut Bitmap,
) -> Result<()> {
    // ANCHOR_END: remove_tree
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: traverse
/// Обходит лес `forest` в поисках листа с координатами `leaf_coordinates`.
/// Возвращает ссылку на запись с номером соответствующего блока данных [`Inode`],
/// хранящуюся в самом [`Inode`] или в его косвенном блоке.
/// При необходимости выделяет косвенные блоки --- узлы дерева --- из `block_bitmap`.
#[allow(unused_mut)] // TODO: remove before flight.
fn traverse<'a>(
    forest: &'a mut Forest,
    leaf_coordinates: LeafCoordinates,
    mut block_bitmap: Option<&mut Bitmap>,
    cache: Cache,
) -> Result<&'a mut usize> {
    // ANCHOR_END: traverse
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: next_level
/// Возвращает срез потомков узла `node` в лесу отображения блоков [`Inode`].
/// При необходимости, то есть когда `*node` равен [`NO_BLOCK`],
/// выделяет косвенный блок для них из `block_bitmap` и заполняет его записями [`NO_BLOCK`].
fn next_level<'a>(
    node: &'a mut usize,
    block_bitmap: &mut Option<&mut Bitmap>,
    cache: Cache,
) -> Result<&'a mut [usize]> {
    // ANCHOR_END: next_level
    // TODO: your code here.
    unimplemented!();
}

/// Координаты листа в лесу.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LeafCoordinates {
    /// Номер листа в его дереве.
    leaf: usize,

    /// Номер дерева в лесу [`Forest`].
    tree: usize,
}

const_assert_eq!(BLOCK_SIZE % mem::size_of::<usize>(), 0);
const_assert_eq!(BLOCK_SIZE % mem::size_of::<Inode>(), 0);

/// Арность дерева отображения блоков.
const INDIRECT_BLOCK_ARITY: usize = BLOCK_SIZE / mem::size_of::<usize>();

// ANCHOR: max_height
/// Максимальная высота дерева отображения блоков.
const MAX_HEIGHT: usize = 4;
// ANCHOR_END: max_height

/// Зарезервированный номер блока, означающий что блок не выделен.
const NO_BLOCK: usize = 0;

#[doc(hidden)]
pub mod test_scaffolding {
    use ku::error::Result;

    use super::super::{
        block_cache::BlockCache,
        test_scaffolding::Bitmap,
    };

    use super::Kind;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Inode(pub(in super::super) super::Inode);

    impl Inode {
        pub fn new(kind: Kind) -> Self {
            Self(super::Inode {
                kind,
                ..Default::default()
            })
        }

        pub fn block_entry(
            &mut self,
            inode_block_number: usize,
            block_bitmap: &mut Bitmap,
        ) -> Result<&mut usize> {
            self.0.block_entry(
                inode_block_number,
                Some(&mut block_bitmap.0),
                BlockCache::cache()?,
            )
        }
    }
}
