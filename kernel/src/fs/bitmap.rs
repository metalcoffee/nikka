use core::ops::Range;

use ku::{
    collections,
    error::{
        Error::{
            Medium,
            NoDisk,
        },
        Result,
    },
    memory::size,
};

use super::{
    BLOCK_SIZE,
    block_cache::BlockCache,
};

// Used in docs.
#[allow(unused)]
use ku::error::Error;

// ANCHOR: bitmap
/// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
/// для отслеживания какие именно элементы
/// (блоки или [inode](https://en.wikipedia.org/wiki/Inode) для соответствующей битовой карты)
/// [файловой системы](https://en.wikipedia.org/wiki/File_system) заняты, а какие --- свободны.
#[derive(Debug)]
pub(super) struct Bitmap {
    /// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap),
    /// каждый элемент этого среза отвечает за [`Self::BITS_PER_ENTRY`] элементов.
    /// Сам срез хранится в памяти блочного кэша [`BlockCache`].
    bitmap: collections::Bitmap<'static>,

    /// Диапазон элементов для пользовательских данных файловой системы.
    /// Полное количество элементов в [файловой системе](https://en.wikipedia.org/wiki/File_system),
    /// то есть количество используемых бит в битовой карте, равно `Bitmap::elements.end`.
    /// Элементы с нулевого до начала этого диапазона (не включительно),
    /// то есть диапазон `0..Bitmap::elements.start`, зарезервированы.
    elements: Range<usize>,
}
// ANCHOR_END: bitmap

impl Bitmap {
    /// Возвращает битовую карту файловой системы или ошибку [`Error::Medium`],
    /// если отведённое под битовую карту место диска содержит данные,
    /// которые не похожи на корректную битовую карту.
    pub(super) fn new(
        block: usize,
        elements: Range<usize>,
    ) -> Result<Self> {
        let bitmap = Self::new_unchecked(Self::bitmap(block, elements.clone()), elements, None);

        bitmap.validate()
    }

    // ANCHOR: format
    /// Форматирует часть диска, начинающуюся с блока `block` и
    /// отведённую под битовую карту для `elements.end` элементов,
    /// его начальным состоянием.
    /// Биты для элементов вне диапазона [`Bitmap::elements`] выставляет в единицы.
    pub(super) fn format(
        block: usize,
        elements: Range<usize>,
    ) -> Result<()> {
        // ANCHOR_END: format
        let bitmap = Self::bitmap(block, elements.clone());

        // TODO: your code here.

        let bitmap = Self::new_unchecked(bitmap, elements.clone(), Some(elements.count()));
        bitmap.validate()?;

        Ok(())
    }

    /// Возвращает `true`, если элемент `number` свободен.
    ///
    /// # Panics
    ///
    /// Паникует, если `number` выходит за пределы диска --- `Bitmap::elements.end`.
    pub(super) fn is_free(
        &self,
        number: usize,
    ) -> bool {
        self.bitmap.is_free(number)
    }

    /// Помечает элемент `number` как свободный.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Значение `number` выходит за пределы файловой системы --- `Bitmap::block_count.end`.
    ///   - Элемент `number` зарезервирован.
    ///   - Элемент `number` уже помечен как свободный.
    pub(super) fn set_free(
        &mut self,
        number: usize,
    ) {
        assert!(self.elements.contains(&number));
        self.bitmap.set_free(number);
    }

    /// Находит по [`Bitmap::bitmap`] свободный элемент и выделяет его.
    /// Возвращает номер выделенного элемента или
    /// ошибку [`Error::NoDisk`], если свободных элементов не осталось.
    pub(super) fn allocate(&mut self) -> Result<usize> {
        self.bitmap.allocate().ok_or(NoDisk)
    }

    /// Возвращает количество свободных элементов в файловой системе.
    pub(super) fn free_count(&self) -> usize {
        self.bitmap.free()
    }

    /// Возвращает количество блоков, которые занимает [`Bitmap`] на диске.
    pub(super) fn size_in_blocks(elements: usize) -> usize {
        elements.div_ceil(size::from(u8::BITS) * BLOCK_SIZE)
    }

    /// Создаёт [`Bitmap`], не проверяя корректность его данных на диске.
    fn new_unchecked(
        bitmap: &'static mut [u64],
        elements: Range<usize>,
        free: Option<usize>,
    ) -> Self {
        Self {
            bitmap: collections::Bitmap::new(bitmap, elements.end, free),
            elements,
        }
    }

    /// Проверяет корректность данных [`Bitmap`] на диске.
    /// Возвращает ошибку [`Error::Medium`],
    /// если данные на диске заведомо не содержат корректный [`Bitmap`],
    /// или сам [`Bitmap`] иначе.
    fn validate(self) -> Result<Self> {
        let len = self.bitmap.len();
        if self.elements.start >= self.elements.end || len < self.elements.end {
            return Err(Medium);
        }

        if (0 .. self.elements.start)
            .chain(self.elements.end .. len)
            .all(|reserved_element| !self.is_free(reserved_element))
        {
            Ok(self)
        } else {
            Err(Medium)
        }
    }

    /// Возвращает срез элементов битовой карты в памяти блочного кэша [`Bitmap`].
    fn bitmap(
        block: usize,
        elements: Range<usize>,
    ) -> &'static mut [u64] {
        unsafe {
            BlockCache::cache()
                .unwrap()
                .block(block)
                .start_address()
                .try_into_mut_slice::<u64>(elements.end.div_ceil(Self::BITS_PER_ENTRY))
                .unwrap()
        }
    }

    /// Количество элементов, за которые отвечает один элемент среза [`Bitmap::bitmap`].
    const BITS_PER_ENTRY: usize = u64::BITS as usize;
}

#[doc(hidden)]
pub mod test_scaffolding {
    use core::ops::Range;

    use ku::error::Result;

    #[derive(Debug)]
    pub struct Bitmap(pub(in super::super) super::Bitmap);

    impl Bitmap {
        pub fn new(
            block: usize,
            elements: Range<usize>,
        ) -> Result<Self> {
            Ok(Self(super::Bitmap::new(block, elements)?))
        }

        pub fn format(
            block: usize,
            elements: Range<usize>,
        ) -> Result<()> {
            super::Bitmap::format(block, elements)
        }

        pub fn is_free(
            &self,
            number: usize,
        ) -> bool {
            self.0.is_free(number)
        }

        pub fn set_free(
            &mut self,
            number: usize,
        ) {
            self.0.set_free(number)
        }

        pub fn allocate(&mut self) -> Result<usize> {
            self.0.allocate()
        }

        pub fn free_count(&self) -> usize {
            self.0.free_count()
        }

        pub fn size_in_blocks(elements: usize) -> usize {
            super::Bitmap::size_in_blocks(elements)
        }
    }
}
