#![allow(rustdoc::private_intra_doc_links)]

use core::{
    cmp,
    hint,
    ops::Add,
};

use heapless::Vec;

use crate::{
    error::{
        Error::InvalidArgument,
        Result,
    },
    memory::size,
};

// Used in docs.
#[allow(unused)]
use crate::error::Error;

// ANCHOR: bitmap
/// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
/// фиксированного размера
/// для отслеживания какие именно элементы заняты, а какие --- свободны.
#[derive(Debug)]
pub struct Bitmap<'a> {
    /// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap),
    /// каждый элемент этого среза отвечает за [`Self::BITS_PER_ENTRY`] элементов.
    bitmap: &'a mut [u64],

    /// Место последнего выделения элемента битовой карты.
    /// Служит для ускорения поиска свободных элементов и равномерного распределения занятых элементов.
    cursor: usize,

    /// Количество свободных элементов.
    free: usize,

    /// Количество элементов.
    len: usize,
}
// ANCHOR_END: bitmap

impl<'a> Bitmap<'a> {
    /// Создаёт битовую карту поверх заданного среза `bitmap`.
    /// Считает, что количество элементов битовой карты равно `len`,
    /// из которых `free` элементов свободны.
    ///
    /// Если `free` равно [`None`], то вычисляет количество свободных элементов.
    /// Иначе, не проверяет, что количество свободных элементов действительно равно `free`.
    /// См. также [`Bitmap::validate()`].
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Срез `bitmap` содержит недостаточно позиций для хранения `len` бит.
    ///   - Количество свободных элементов `free` превышает полное количество элементов `len`.
    pub fn new<'b: 'a>(
        bitmap: &'b mut [u64],
        len: usize,
        free: Option<usize>,
    ) -> Self {
        let entry_count = len.div_ceil(Self::BITS_PER_ENTRY);
        let free = free.unwrap_or_else(|| Self::count_free(bitmap, len));

        let mut cursor = (len - free) / Self::BITS_PER_ENTRY;
        if cursor >= entry_count {
            cursor = 0;
        }

        assert!(entry_count <= bitmap.len());
        assert!(free <= len);

        Self {
            bitmap: &mut bitmap[.. entry_count],
            cursor,
            free,
            len,
        }
    }

    /// Возвращает пустой [`Bitmap`].
    pub const fn zero() -> Self {
        Self {
            bitmap: &mut [],
            cursor: 0,
            free: 0,
            len: 0,
        }
    }

    /// Возвращает количество свободных элементов.
    pub fn free(&self) -> usize {
        self.free
    }

    /// Возвращает полное количество элементов.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Возвращает `true`, если битовая карта пуста, то есть [`Bitmap::len()`] равно нулю.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Возвращает `true`, если элемент `number` свободен.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Значение `number` больше или равно размеру --- [`Bitmap::len()`].
    pub fn is_free(
        &self,
        number: usize,
    ) -> bool {
        debug_assert!(number < self.len());
        !Self::bit(self.bitmap, number)
    }

    /// Помечает элемент `number` как свободный.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Значение `number` больше или равно размеру --- [`Bitmap::len()`].
    ///   - Элемент `number` уже помечен как свободный.
    pub fn set_free(
        &mut self,
        number: usize,
    ) {
        debug_assert!(number < self.len());
        debug_assert!(!self.is_free(number));
        self.bitmap[number / Self::BITS_PER_ENTRY] &= !(1 << (number % Self::BITS_PER_ENTRY));
        self.free += 1;
    }

    // ANCHOR: allocate
    /// Находит в битовой карте свободный элемент и помечает его занятым.
    /// Возвращает номер выделенного элемента или [`None`], если свободных элементов не осталось.
    ///
    /// При поиске свободного элемента стартует с позиции [`Bitmap::cursor`] в
    /// срезе [`Bitmap::bitmap`] и запоминает в [`Bitmap::cursor`] позицию,
    /// из которого выделен свободный элемент.
    pub fn allocate(&mut self) -> Option<usize> {
        // ANCHOR_END: allocate
        if self.free == 0 {
            return None;
        }
        
        let bitmap_len = self.bitmap.len();
        let start_cursor = self.cursor;
        for offset in 0..bitmap_len {
            let index = (start_cursor + offset) % bitmap_len;
            let entry = self.bitmap[index];
            if entry != u64::MAX {
                let bit_position = entry.trailing_ones() as usize;
                let element_number = index * Self::BITS_PER_ENTRY + bit_position;
                if element_number < self.len {
                    self.bitmap[index] |= 1 << bit_position;
                    self.cursor = index;
                    self.free -= 1;
                    
                    return Some(element_number);
                }
            }
        }
        unsafe { hint::unreachable_unchecked() }
    }

    // ANCHOR: bulk_allocate
    /// Аналогична [`Bitmap::allocate()`], но выделяет за один раз несколько элементов.
    /// Если свободных элементов не хватает, то выделенных элементов может вернуться
    /// меньше, чем `count` --- запрошенное количество элементов.
    ///
    /// При поиске свободного элемента стартует с позиции [`Bitmap::cursor`] в
    /// срезе [`Bitmap::bitmap`] и запоминает в [`Bitmap::cursor`] позицию,
    /// из которого выделен свободный элемент.
    ///
    /// # Panics
    ///
    /// Паникует, если `count` превышает `SIZE`, --- резервируемый размер под результат.
    pub fn bulk_allocate<const SIZE: usize>(
        &mut self,
        count: usize,
    ) -> Vec<usize, SIZE> {
        // ANCHOR_END: bulk_allocate
        assert!(count <= SIZE);

        let mut result = Vec::new();
        let to_allocate = cmp::min(count, self.free);
        
        for _ in 0..to_allocate {
            if let Some(element) = self.allocate() {
                unsafe {
                    result.push_unchecked(element);
                }
            } else {
                break;
            }
        }
        
        result
    }

    /// Проверяет корректность поля [`Bitmap::free`].
    ///
    /// Возвращает ошибку:
    ///   - [`Error::InvalidArgument`] если количество свободных битов в
    ///     [`Bitmap::bitmap`] не совпадает с [`Bitmap::free`].
    pub fn validate(&self) -> Result<()> {
        if self.free == Self::count_free(self.bitmap, self.len) {
            Ok(())
        } else {
            Err(InvalidArgument)
        }
    }

    /// Вычисляет количество свободных элементов в `bitmap`.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Срез `bitmap` содержит недостаточно позиций для хранения `len` бит.
    fn count_free(
        bitmap: &[u64],
        len: usize,
    ) -> usize {
        assert!(len.div_ceil(Self::BITS_PER_ENTRY) <= bitmap.len());

        let full_elements_count = len / Self::BITS_PER_ENTRY;

        let full_elements_free_count = bitmap[.. full_elements_count]
            .iter()
            .map(|bits| bits.count_zeros().try_into().unwrap())
            .reduce(Add::add)
            .unwrap_or(0);

        let last_element_free_count = (0 .. len % Self::BITS_PER_ENTRY)
            .map(|number| {
                if Self::bit(bitmap, full_elements_count * Self::BITS_PER_ENTRY + number) {
                    0
                } else {
                    1
                }
            })
            .reduce(Add::add)
            .unwrap_or(0);

        full_elements_free_count + last_element_free_count
    }

    /// Возвращает `true` если в битовой карте `bitmap` установлен бит номер `number`.
    fn bit(
        bitmap: &[u64],
        number: usize,
    ) -> bool {
        bitmap[number / Self::BITS_PER_ENTRY] & (1 << (number % Self::BITS_PER_ENTRY)) != 0
    }

    /// Количество элементов, за которые отвечает один элемент среза [`Bitmap::bitmap`].
    pub const BITS_PER_ENTRY: usize = u64::BITS as usize;
}

#[cfg(test)]
mod test {
    use super::Bitmap;

    #[test]
    fn count_free() {
        let bitmap = [0; 3];
        for len in 0 .. bitmap.len() * Bitmap::BITS_PER_ENTRY {
            assert_eq!(Bitmap::count_free(&bitmap, len), len);
        }

        let bitmap = [u64::MAX; 3];
        for len in 0 .. bitmap.len() * Bitmap::BITS_PER_ENTRY {
            assert_eq!(Bitmap::count_free(&bitmap, len), 0);
        }

        let free = [7, Bitmap::BITS_PER_ENTRY - 1, 0];
        let bitmap = [
            u64::MAX - (1 << free[0]),
            u64::MAX - (1 << free[1]),
            u64::MAX - (1 << free[2]),
        ];
        for len in 0 .. bitmap.len() * Bitmap::BITS_PER_ENTRY {
            let free_count = free
                .iter()
                .enumerate()
                .filter(|&(i, &j)| i * Bitmap::BITS_PER_ENTRY + j < len)
                .count();
            assert_eq!(Bitmap::count_free(&bitmap, len), free_count);
        }

        let bitmap = [0b_1101_1010; 1];
        assert_eq!(Bitmap::count_free(&bitmap, 1), 1);
        assert_eq!(Bitmap::count_free(&bitmap, 2), 1);
        assert_eq!(Bitmap::count_free(&bitmap, 3), 2);
        assert_eq!(Bitmap::count_free(&bitmap, 4), 2);
        assert_eq!(Bitmap::count_free(&bitmap, 5), 2);
        assert_eq!(Bitmap::count_free(&bitmap, 6), 3);
        assert_eq!(Bitmap::count_free(&bitmap, 7), 3);
        assert_eq!(Bitmap::count_free(&bitmap, 8), 3);
    }
}
