use core::{
    alloc::Layout,
    cmp,
    hint,
    mem,
    ptr,
};

use crate::{
    allocator::BigAllocatorGuard,
    collections::DynamicBitmap,
    error::Result,
    log::{
        error,
        trace,
    },
    memory::Virt,
    sync::Spinlock,
};

use super::{
    CLIP_SIZE,
    Clip,
    Info,
    Quarry,
};

/// Аллокатор, выделяющий память блоками одинакового размера.
///
/// Предполагает, что все блоки памяти, с которыми он работает,
/// имеют одинаковый размер.
/// Более того, этот размер и выравнивание блоков должны быть кратны [`MIN_SIZE`].
#[derive(Debug)]
pub(super) struct FixedSizeAllocator {
    /// занятости блоков из [`FixedSizeAllocator::quarry`].
    bitmap: DynamicBitmap,

    /// Статистика аллокатора.
    info: Info,

    /// Хранилище для блоков памяти фиксированного размера.
    quarry: Option<Quarry>,
}

impl FixedSizeAllocator {
    /// Создаёт аллокатор, выделяющий память блоками одинакового размера.
    pub(super) const fn new() -> Self {
        Self {
            bitmap: DynamicBitmap::new(),
            info: Info::new(),
            quarry: None,
        }
    }

    /// Статистика аллокатора.
    pub(super) fn info(&self) -> &Info {
        &self.info
    }

    /// Статистика аллокатора.
    pub(super) fn info_mut(&mut self) -> &mut Info {
        &mut self.info
    }

    /// Возвращает `true`, если у аллокатора больше нет свободных блоков.
    pub(super) fn is_empty(&self) -> bool {
        self.bitmap.free() == 0
    }

    // ANCHOR: allocate
    /// Выделяет свободный блок памяти из [`FixedSizeAllocator::quarry`].
    /// Возвращает указатель на выделенный блок или
    /// [`core::ptr::null_mut()`], если свободных блоков не осталось.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - [`FixedSizeAllocator`] не подходит для операции,
    ///     которую описывает `layout`.
    #[allow(unused)]
    pub(super) fn allocate(
        &mut self,
        layout: Layout,
    ) -> *mut u8 {
        // ANCHOR_END: allocate
        self.validate_layout(layout);

        if let Some(index) = self.bitmap.allocate() {
            let address = self.quarry().allocation(index);
            
            if address == Virt::default() {
                self.bitmap.set_free(index);
                return ptr::null_mut();
            }
            
            address.into_usize() as *mut u8
        } else {
            ptr::null_mut()
        }
    }

    // ANCHOR: deallocate
    /// Освобождает выделенные ранее методом [`FixedSizeAllocator::allocate()`]
    /// блок памяти, начинающийся по указателю `ptr`.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать
    /// [то же самое](https://doc.rust-lang.org/nightly/core/alloc/trait.Allocator.html#safety-1),
    /// что требуется при вызове [`core::alloc::Allocator::deallocate()`].
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Блок уже является свободным.
    ///   - Блок не попадает в [`FixedSizeAllocator::quarry`].
    ///   - [`FixedSizeAllocator`] не подходит для операции,
    ///     которую описывает `layout`.
    pub(super) unsafe fn deallocate(
        &mut self,
        ptr: *mut u8,
        layout: Layout,
    ) {
        // ANCHOR_END: deallocate
        assert!(!ptr.is_null());
        self.validate_layout(layout);

        unsafe {
            self.deallocate_impl(ptr);
        }
    }

    // ANCHOR: fill_clip
    /// Заполняет `clip` до тех пор, пока либо в нём не будет `len` элементов,
    /// либо не исчерпаются свободные элементы [`FixedSizeAllocator`].
    pub(super) fn fill_clip(
        &mut self,
        clip: &mut Clip,
        len: usize,
        allocator_lock: &Spinlock<FixedSizeAllocator>,
    ) {
        // ANCHOR_END: fill_clip
        let count = len.saturating_sub(clip.len());
        if count == 0 {
            return;
        }
        
        // Bind clip on first use (when it's empty and has capacity)
        // Once bound, the clip stays bound even when empty
        if !clip.is_bound() && clip.capacity() > 0 {
            unsafe {
                clip.bind_unchecked(allocator_lock);
            }
        }
        
        let indices = self.bitmap.bulk_allocate::<CLIP_SIZE>(count);
        
        for index in indices.iter() {
            let address = self.quarry().allocation(*index);
            
            if address == Virt::default() {
                self.bitmap.set_free(*index);
                continue;
            }
            
            let ptr = address.into_usize() as *mut u8;
            unsafe {
                clip.push_unchecked(ptr);
            }
        }
    }

    // ANCHOR: unfill_clip
    /// Освобождает `clip`, пока в нём не останется не более `len` блоков.
    pub(super) fn unfill_clip(
        &mut self,
        clip: &mut Clip,
        len: usize,
    ) {
        // ANCHOR_END: unfill_clip
        while clip.len() > len {
            if let Some(ptr) = clip.pop() {
                unsafe {
                    self.deallocate_impl(ptr);
                }
            } else {
                break;
            }
        }
    }

    /// Выделяет из `fallback` некоторое количество новых блоков размера `size`.
    /// При необходимости, расширяет память под свои метаданные ---
    /// [`FixedSizeAllocator::bitmap`].
    ///
    /// Возвращает количество физических фреймов,
    /// которые пришлось выделить из `fallback` и под запрошенные блоки и под метаданные.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - `size` не подходит под требования размера [`FixedSizeAllocator`].
    ///   - `size` не равен ранее передававшемуся в этот метод значению.
    pub(super) fn stock_up(
        &mut self,
        size: usize,
        fallback: &impl BigAllocatorGuard,
    ) -> Result<usize> {
        assert_eq!(size % MIN_SIZE, 0);

        let mut total_frames = 0;
        
        if self.quarry.is_none() {
            self.quarry = Some(Quarry::new(size));
        }
        
        let quarry = self.quarry.as_mut().unwrap();
        assert_eq!(quarry.size(), size, "size mismatch in stock_up");
        
        let old_allocation_count = self.bitmap.len();
        let capacity = quarry.capacity();
        
        let new_allocation_count = cmp::min(old_allocation_count + CLIP_SIZE, capacity);
        
        if new_allocation_count == old_allocation_count {
            return Ok(0);
        }
        
        if self.bitmap.is_empty() {
            self.bitmap.reserve(capacity, Self::BITMAP_GUARD_ZONE_PAGE_COUNT, fallback)?;
        }
        
        if new_allocation_count > self.bitmap.len() {
            let bitmap_frames = self.bitmap.map(new_allocation_count, fallback)?;
            total_frames += bitmap_frames;
        }
        
        let mut actual_allocation_count = new_allocation_count;
        let quarry_frames = quarry.map(old_allocation_count, &mut actual_allocation_count, fallback)?;
        total_frames += quarry_frames;
        
        if total_frames > 0 {
            self.info.pages_allocation(total_frames);
        }
        
        trace!(
            "FixedSizeAllocator::stock_up: size={}, old_count={}, new_count={}, actual_count={}, frames={}",
            size,
            old_allocation_count,
            new_allocation_count,
            actual_allocation_count,
            total_frames
        );
        
        Ok(total_frames)
    }

    // ANCHOR: unmap
    /// Освобождает как всю использованную [`FixedSizeAllocator`] физическую память,
    /// так и всё использованное им адресное пространство.
    /// Возвращает их в страничный аллокатор `fallback`.
    /// Переводит [`FixedSizeAllocator::bitmap`] и [`FixedSizeAllocator::quarry`]
    /// в не инициализированное состояние,
    /// такое же как создаваемое методом [`FixedSizeAllocator::new()`].
    /// Но накопленные статистики [`FixedSizeAllocator::info`] при этом не сбрасываются.
    ///
    /// Возвращает количество освобождённых физических фреймов.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Не все блоки этого [`FixedSizeAllocator`] освобождены.
    pub fn unmap(
        &mut self,
        fallback: &impl BigAllocatorGuard,
    ) -> Result<usize> {
        // ANCHOR_END: unmap
        assert_eq!(self.info.allocations().balance(), 0, "Not all blocks are freed");
        assert_eq!(self.info.allocated().balance(), 0, "Not all blocks are freed");
        assert_eq!(self.info.requested().balance(), 0, "Not all blocks are freed");

        let mut total_frames = 0;
        
        if let Some(ref mut quarry) = self.quarry {
            let allocation_count = self.bitmap.len();
            let quarry_frames = quarry.unmap(allocation_count, fallback)?;
            total_frames += quarry_frames;
        }
        
        if self.bitmap.len() > 0 {
            let bitmap_frames = self.bitmap.unmap(fallback)?;
            total_frames += bitmap_frames;
        }
        
        if total_frames > 0 {
            self.info.pages_deallocation(total_frames);
        }
        
        self.quarry = None;
        
        Ok(total_frames)
    }

    /// Освобождает выделенные ранее методом [`FixedSizeAllocator::allocate()`]
    /// блок памяти, начинающийся по указателю `ptr`.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать
    /// [то же самое](https://doc.rust-lang.org/nightly/core/alloc/trait.Allocator.html#safety-1),
    /// что требуется при вызове [`core::alloc::Allocator::deallocate()`].
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Блок уже является свободным.
    ///   - Блок не попадает в [`FixedSizeAllocator::quarry`].
    pub(super) unsafe fn deallocate_impl(
        &mut self,
        ptr: *mut u8,
    ) {
        debug_assert!(!ptr.is_null());

        let address = Virt::from_ptr(ptr);
        
        let index = self.quarry().allocation_index(address)
            .expect("pointer not found in quarry");
        
        assert!(!self.bitmap.is_free(index), "double free detected");
        
        self.bitmap.set_free(index);
    }

    /// Возвращает хранилище блоков памяти фиксированного размера.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - [`FixedSizeAllocator::quarry`] ещё не инициализирован,
    ///     то есть ещё не было сделано ни одной операции выделения или освобождения памяти.
    fn quarry(&mut self) -> &mut Quarry {
        self.quarry.as_mut().expect(Self::UNINITIALIZED_MESSAGE)
    }

    /// Проверяет, что [`FixedSizeAllocator`] для размера `size`
    /// поддерживает выделение блоков памяти заданного `layout`.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - [`FixedSizeAllocator`] не подходит для операции,
    ///     которую описывает `layout`.
    fn validate_layout(
        &self,
        layout: Layout,
    ) {
        let size = self.quarry.as_ref().expect(Self::UNINITIALIZED_MESSAGE).size();
        debug_assert!(layout.size() <= size);
        debug_assert_eq!(size % layout.align(), 0);
    }

    /// Количество страниц в каждой из не отображённыx в память защитных зон,
    /// которыми битовая карта [`FixedSizeAllocator::bitmap`]
    /// окружается в адресном пространстве с обоих сторон.
    /// Это необходимо, чтобы защитить метаданные аллокатора от перезаписи неверным
    /// использующим аллокатор кодом, обращающимся за пределы выданного ему блока памяти.
    const BITMAP_GUARD_ZONE_PAGE_COUNT: usize = 1;

    /// Сообщение для паник, когда [`FixedSizeAllocator`] ещё не инициализирован.
    const UNINITIALIZED_MESSAGE: &str = "FixedSizeAllocator is not initialized";
}

impl Drop for FixedSizeAllocator {
    fn drop(&mut self) {
        // After unmap() is called, bitmap should have len == 0 and quarry should be None
        // is_empty() checks if len == 0, which is what we want
        assert_eq!(self.bitmap.len(), 0, "FixedSizeAllocator dropped with non-empty bitmap");
        assert!(self.quarry.is_none(), "FixedSizeAllocator dropped with quarry still initialized");
    }
}

/// Максимальный тип, подходящий для зануления и копирования блоков памяти,
/// которыми управляет [`FixedSizeAllocator`].
pub(super) type MaxPrimitiveType = u64;

/// Минимальный размер и выравнивание для блоков памяти,
/// которыми управляет [`FixedSizeAllocator`].
pub(super) const MIN_SIZE: usize = mem::size_of::<MaxPrimitiveType>();
