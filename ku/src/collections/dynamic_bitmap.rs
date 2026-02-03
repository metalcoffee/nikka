use derive_more::{
    Deref,
    DerefMut,
};

use crate::{
    allocator::{
        BigAllocator,
        BigAllocatorGuard,
    },
    error::{
        Error::{
            InvalidAlignment,
            PermissionDenied,
        },
        Result,
    },
    memory::{
        Block,
        Page,
        size,
    },
};

use super::Bitmap;

// Used in docs.
#[allow(unused)]
use crate::error::Error;

// ANCHOR: bitmap
/// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
/// расширяемого размера
/// для отслеживания какие именно элементы заняты, а какие --- свободны.
#[derive(Debug, Deref, DerefMut)]
pub struct DynamicBitmap {
    /// Битовая карта текущего фиксированного размера.
    #[deref]
    #[deref_mut]
    bitmap: Bitmap<'static>,

    /// Количество страниц в каждой из не отображённыx в память защитных зон,
    /// которыми битовая карта окружается в адресном пространстве с обоих сторон.
    guard_zone_page_count: usize,

    /// Зарезервированный под битовую карту и её защитные зоны блок страниц.
    /// В память отображена только часть этого блока,
    /// достаточная для хранения [`DynamicBitmap::bitmap`] текущего размера.
    pages: Block<Page>,
}
// ANCHOR_END: bitmap

#[allow(rustdoc::private_intra_doc_links)]
impl DynamicBitmap {
    /// Возвращает пустой [`DynamicBitmap`].
    /// В отличие от [`DynamicBitmap::default()`] доступна в константном контексте.
    pub const fn new() -> Self {
        Self {
            bitmap: Bitmap::zero(),
            guard_zone_page_count: 0,
            pages: Block::zero(),
        }
    }

    // ANCHOR: reserve
    /// С помощью `allocator` резервирует в адресном пространстве
    /// блок страниц [`DynamicBitmap::pages`], достаточный для хранения
    /// битовой карты [`DynamicBitmap::bitmap`] на `capacity` элементов (бит).
    ///
    /// Битовая карта окружается в адресном пространстве с обоих сторон не отображёнными в память
    /// защитными зонами размера `guard_zone_page_count` страниц.
    ///
    /// Возвращает ошибку:
    ///   - [`Error::NoPage`] если выделить необходимый размер виртуальной памяти не удалось.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Метод [`DynamicBitmap::reserve()`] уже вызывался, то есть [`DynamicBitmap`] не пуст.
    ///   - Ёмкость `capacity == 0`.
    pub fn reserve(
        &mut self,
        capacity: usize,
        guard_zone_page_count: usize,
        allocator: &impl BigAllocatorGuard,
    ) -> Result<()> {
        assert!(self.pages.is_empty());
        assert!(capacity > 0);
        
        let bitmap_page_count = Self::page_count(capacity);
        let total_page_count = guard_zone_page_count + bitmap_page_count + guard_zone_page_count;
        
        let mut alloc = allocator.get();
        let layout = Page::layout_array(total_page_count);
        let pages = alloc.reserve(layout)?;
        
        self.pages = pages;
        self.guard_zone_page_count = guard_zone_page_count;
        
        Ok(())
    }

    // ANCHOR: map
    /// С помощью `allocator` отображает в память
    /// часть блока страниц [`DynamicBitmap::pages`], достаточную для хранения
    /// битовой карты [`DynamicBitmap::bitmap`] на `new_len` элементов (бит).
    /// Новые элементы битовой карты инициализируются нулями.
    /// Страницы отображаются с флагами `allocator.flags()`.
    ///
    /// Возвращает количество выделенных физических фреймов памяти.
    ///
    /// Возвращает ошибку:
    ///   - [`Error::NoFrame`] если свободных физических фреймов не осталось.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Новый размер `new_len` не превышает старый размер [`DynamicBitmap::bitmap`].
    pub fn map(
        &mut self,
        new_len: usize,
        allocator: &impl BigAllocatorGuard,
    ) -> Result<usize> {
        assert!(new_len > self.len());
        
        let old_len = self.len();
        let old_free = self.free();
        let old_page_count = Self::page_count(old_len);
        let new_page_count = Self::page_count(new_len);
        let additional_page_count = new_page_count.saturating_sub(old_page_count);
        
        if additional_page_count > 0 {
            let bitmap_pages = self.pages();
            let pages_to_map = bitmap_pages
                .slice(old_page_count .. new_page_count)
                .expect("err");
            
            let mut alloc = allocator.get();
            unsafe {
                alloc.map(pages_to_map, alloc.flags())?;
                
                let new_pages_slice = pages_to_map
                    .try_into_mut_slice::<u8>()
                    .expect("err");
                new_pages_slice.fill(0);
            }
        }
        
        let bitmap_pages = self.pages();
        let bitmap_slice = unsafe {
            bitmap_pages
                .slice(0 .. new_page_count)
                .expect("err")
                .try_into_mut_slice::<u64>()
                .expect("err")
        };
        
        let new_free = old_free + (new_len - old_len);
        self.bitmap = Bitmap::new(bitmap_slice, new_len, Some(new_free));
        
        Ok(additional_page_count)
    }

    // ANCHOR: unmap
    /// С помощью `allocator`:
    ///   - Удаляет отображение в память битовой карты [`DynamicBitmap::bitmap`].
    ///   - Разрезервирует блок [`DynamicBitmap::pages`],
    ///     который был ранее зарезервирован в адресном пространстве.
    ///
    /// После этого присваивает `self` пустое значение,
    /// которое возвращает [`DynamicBitmap::new()`].
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Не все элементы свободны,
    ///     то есть для поля [`DynamicBitmap::bitmap`] значения
    ///     [`Bitmap::free()`] и [`Bitmap::len()`] различаются.
    pub fn unmap(
        &mut self,
        allocator: &impl BigAllocatorGuard,
    ) -> Result<usize> {
        // ANCHOR_END: unmap
        assert_eq!(self.free(), self.len());

        let old_len = self.len();
        let page_count = Self::page_count(old_len);
        
        let bitmap_pages = self.pages();
        let mapped_pages = bitmap_pages
            .slice(0 .. page_count)
            .expect("err");
        
        let mut alloc = allocator.get();
        unsafe {
            alloc.unmap(mapped_pages)?;
        }
        
        unsafe {
            alloc.unreserve(self.pages)?;
        }
        
        *self = Self::new();
        
        Ok(page_count)
    }

    /// Возвращает количество страниц, которые занимает битовая карта [`DynamicBitmap::bitmap`]
    /// на `len` элементов (бит).
    fn page_count(len: usize) -> usize {
        len.div_ceil(size::from(u8::BITS) * Page::SIZE)
    }

    /// Возвращает зарезервированный под битовую карту блок страниц.
    fn pages(&self) -> Block<Page> {
        self.pages
            .slice(self.guard_zone_page_count .. self.pages.count() - self.guard_zone_page_count)
            .expect("failed to make guard zones")
    }
}

impl Default for DynamicBitmap {
    fn default() -> Self {
        Self::new()
    }
}
