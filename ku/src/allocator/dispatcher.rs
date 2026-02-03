use core::{
    alloc::{
        Allocator,
        GlobalAlloc,
        Layout,
    },
    cmp,
    fmt,
    hint,
    intrinsics,
    ptr::{
        self,
        NonNull,
    },
};

use crate::{
    error::Result,
    memory::Page,
    sync::Spinlock,
};

use super::{
    BigAllocatorGuard,
    cache::Cache,
    dry::Initialize,
    fixed_size::{
        FixedSizeAllocator,
        MIN_SIZE,
        MaxPrimitiveType,
    },
    info::{
        AtomicInfo,
        Info,
    },
};

/// Аллокатор верхнего уровня.
/// По запрошенному размеру определяет из какого аллокатора будет выделяться память.
#[derive(Debug)]
pub struct Dispatcher<Q: Cache, T: Allocator + BigAllocatorGuard> {
    /// Кэш выделяемых блоков.
    cache: Q,

    /// Аллокатор для выделения памяти постранично, в том числе остальным аллокаторам.
    fallback: T,

    /// Статистика выделений и освобождений через [`Dispatcher::fallback`].
    fallback_info: AtomicInfo,

    /// Набор аллокаторов для разных размеров блоков.
    /// Каждый из них выделяет память блоками фиксированного размера.
    fixed_size: [Spinlock<FixedSizeAllocator>; FIXED_SIZE_COUNT],

    /// Общая статистика аллокатора.
    info: AtomicInfo,
}

impl<Q: Cache, T: Allocator + BigAllocatorGuard> Dispatcher<Q, T> {
    /// Создаёт аллокатор верхнего уровня с заданным постраничным аллокатором `fallback`.
    pub const fn new(
        cache: Q,
        fallback: T,
    ) -> Self {
        Self {
            cache,
            fallback,
            fallback_info: AtomicInfo::new(),
            fixed_size: [FIXED_SIZE_DUMMY; FIXED_SIZE_COUNT],
            info: AtomicInfo::new(),
        }
    }

    /// Общая статистика аллокатора.
    pub fn info(&self) -> Info {
        self.info.load()
    }

    /// Учитывает в счётчиках выделение `allocated_pages` виртуальных страниц.
    pub fn pages_allocation(
        &self,
        allocated_pages: usize,
    ) {
        self.info.pages_allocation(allocated_pages);
    }

    /// Учитывает в счётчиках освобождение `deallocated_pages` виртуальных страниц.
    pub fn pages_deallocation(
        &self,
        deallocated_pages: usize,
    ) {
        self.info.pages_deallocation(deallocated_pages);
    }

    /// Записывает в `detailed_info` статистику аллокатора.
    /// Как суммарную, так и с разбивкой по компонентам.
    ///
    /// В случае конкурентных запросов к аллокатору статистика может быть несогласованной.
    pub fn detailed_info(
        &self,
        detailed_info: &mut DetailedInfo,
    ) {
        detailed_info.total = self.info();
        detailed_info.fallback = self.fallback_info.load();

        for (info, allocator) in detailed_info.fixed_size.iter_mut().zip(&self.fixed_size) {
            *info = *allocator.lock().info();
        }
    }

    // ANCHOR: unmap
    /// Освобождает всю виртуальную и физическую память, выделенную аллокатором.
    ///
    /// # Panics
    ///
    /// Паникует, если остались не освобождённые блоки памяти.
    pub fn unmap(&self) {
        // ANCHOR_END: unmap
        let info = self.info();
        assert_eq!(info.allocations().balance(), 0, "Not all blocks are freed");
        assert_eq!(info.allocated().balance(), 0, "Not all blocks are freed");
        assert_eq!(info.requested().balance(), 0, "Not all blocks are freed");

        let fallback_info = self.fallback_info.load();
        assert_eq!(fallback_info.allocations().balance(), 0, "Fallback has unfreed blocks");
        assert_eq!(fallback_info.allocated().balance(), 0, "Fallback has unfreed blocks");
        assert_eq!(fallback_info.requested().balance(), 0, "Fallback has unfreed blocks");

        // Empty all clips before unmapping allocators
        if Q::CACHE_AVAILABLE {
            for index in 0..FIXED_SIZE_COUNT {
                self.cache.with_borrow_mut(index, |clip| {
                    if !clip.is_empty() {
                        let mut allocator = self.fixed_size[index].lock();
                        allocator.unfill_clip(clip, 0);
                    }
                });
            }
        }

        // Unmap all fixed size allocators
        for allocator in &self.fixed_size {
            let mut alloc = allocator.lock();
            if let Ok(freed_pages) = alloc.unmap(&self.fallback) {
                self.info.pages_deallocation(freed_pages);
            }
        }

        assert_eq!(self.info().pages().balance(), 0, "Not all pages are freed");
    }

    /// Находит индекс [`FixedSizeAllocator`], который отвечает за заданный `layout`.
    ///
    /// Возвращает [`None`], если такого нет.
    /// То есть, если за этот `layout` отвечает [`Dispatcher::fallback`].
    fn get_fixed_size_index(
        &self,
        layout: Layout,
    ) -> Option<usize> {
        let size = layout.size().next_multiple_of(MIN_SIZE);
        let align = layout.align().next_multiple_of(MIN_SIZE);
        
        let mut required_size = cmp::max(size, align);
        required_size = required_size.next_multiple_of(align);
        
        if required_size % Page::SIZE == 0 || required_size > FIXED_SIZE_COUNT * MIN_SIZE {
            return None;
        }
        
        let index = required_size / MIN_SIZE - 1;
        Some(index)
    }

    /// Возвращает размер блоков памяти, за которые отвечает аллокатор
    /// по индексу `index` в массиве [`Dispatcher::fixed_size`].
    #[allow(unused)]
    fn get_size(index: usize) -> usize {
        get_size(index)
    }

    /// Выделяет блок памяти с помощью аллокатора `fallback`.
    fn fallback_allocate(
        &self,
        layout: Layout,
        initialize: Initialize,
    ) -> *mut u8 {
        let ptr = match initialize {
            Initialize::Garbage => self.fallback.allocate(layout),
            Initialize::Zero => self.fallback.allocate_zeroed(layout),
        };

        if let Ok(ptr) = ptr {
            self.update_fallback_info(Operation::Allocation, layout);

            ptr.as_non_null_ptr().as_ptr()
        } else {
            ptr::null_mut()
        }
    }

    // ANCHOR: fixed_size_allocate
    /// Выделяет блок памяти с помощью аллокатора
    /// по индексу `index` в массиве [`Dispatcher::fixed_size`].
    fn fixed_size_allocate(
        &self,
        index: usize,
        layout: Layout,
    ) -> *mut u8 {
        // ANCHOR_END: fixed_size_allocate
        if Q::CACHE_AVAILABLE {
            // Try to allocate from cache first
            let ptr = self.cache.with_borrow_mut(index, |clip| {
                clip.pop()
            });
            
            if let Some(ptr) = ptr {
                // Fast path: allocated from cache, no lock needed for statistics
                #[cfg(feature = "allocator-statistics")]
                {
                    let size = Self::get_size(index);
                    let mut allocator = self.fixed_size[index].lock();
                    allocator.info_mut().allocation(layout.size(), size, 0);
                    self.info.allocation(layout.size(), size, 0);
                }
                return ptr;
            }
            
            // Cache is empty, need to refill it
            let mut allocator = self.fixed_size[index].lock();
            
            // Stock up if needed
            if allocator.is_empty() {
                let size = Self::get_size(index);
                match allocator.stock_up(size, &self.fallback) {
                    Ok(allocated_pages) => {
                        if allocated_pages > 0 {
                            self.info.pages_allocation(allocated_pages);
                        }
                    },
                    Err(_) => return ptr::null_mut(),
                }
            }
            
            self.cache.with_borrow_mut(index, |clip| {
                // Fill clip to half capacity
                let target_len = clip.capacity() / 2;
                allocator.fill_clip(clip, target_len, &self.fixed_size[index]);
                
                // Try to allocate again
                if let Some(ptr) = clip.pop() {
                    #[cfg(feature = "allocator-statistics")]
                    {
                        let size = Self::get_size(index);
                        allocator.info_mut().allocation(layout.size(), size, 0);
                        self.info.allocation(layout.size(), size, 0);
                    }
                    ptr
                } else {
                    ptr::null_mut()
                }
            })
        } else {
            // No cache available, allocate directly
            let mut allocator = self.fixed_size[index].lock();
            
            if allocator.is_empty() {
                let size = Self::get_size(index);
                match allocator.stock_up(size, &self.fallback) {
                    Ok(allocated_pages) => {
                        if allocated_pages > 0 {
                            self.info.pages_allocation(allocated_pages);
                        }
                    },
                    Err(_) => return ptr::null_mut(),
                }
            }
            
            let ptr = allocator.allocate(layout);
            
            #[cfg(feature = "allocator-statistics")]
            if !ptr.is_null() {
                let size = Self::get_size(index);
                allocator.info_mut().allocation(layout.size(), size, 0);
                self.info.allocation(layout.size(), size, 0);
            }
            
            ptr
        }
    }

    // ANCHOR: fixed_size_deallocate
    /// Освобождает блок памяти с помощью аллокатора
    /// по индексу `index` в массиве [`Dispatcher::fixed_size`].
    ///
    /// # Safety
    ///
    /// - `ptr` должен быть ранее выделен через [`Dispatcher::fixed_size_allocate()`]
    ///   с теми же `index` и `layout`.
    /// - Память, описываемая `ptr` и `layout` больше не должна использоваться.
    unsafe fn fixed_size_deallocate(
        &self,
        index: usize,
        ptr: *mut u8,
        layout: Layout,
    ) {
        // ANCHOR_END: fixed_size_deallocate
        if Q::CACHE_AVAILABLE {
            // Try to return to cache first
            let result = self.cache.with_borrow_mut(index, |clip| {
                if clip.len() < clip.capacity() {
                    // Successfully cached
                    unsafe {
                        clip.push_unchecked(ptr);
                    }
                    true
                } else {
                    // Cache is full
                    false
                }
            });
            
            if result {
                // Fast path: returned to cache, no lock needed for statistics
                #[cfg(feature = "allocator-statistics")]
                {
                    let size = Self::get_size(index);
                    let mut allocator = self.fixed_size[index].lock();
                    allocator.info_mut().deallocation(layout.size(), size, 0);
                    self.info.deallocation(layout.size(), size, 0);
                }
                return;
            }
            
            // Cache is full, need to unfill it
            let mut allocator = self.fixed_size[index].lock();
            
            self.cache.with_borrow_mut(index, |clip| {
                // Unfill clip to half capacity
                let target_len = clip.capacity() / 2;
                allocator.unfill_clip(clip, target_len);
                
                // Try to cache again
                if clip.len() < clip.capacity() {
                    unsafe {
                        clip.push_unchecked(ptr);
                    }
                    #[cfg(feature = "allocator-statistics")]
                    {
                        let size = Self::get_size(index);
                        allocator.info_mut().deallocation(layout.size(), size, 0);
                        self.info.deallocation(layout.size(), size, 0);
                    }
                } else {
                    // Still can't cache, deallocate directly
                    unsafe {
                        allocator.deallocate(ptr, layout);
                    }
                    #[cfg(feature = "allocator-statistics")]
                    {
                        let size = Self::get_size(index);
                        allocator.info_mut().deallocation(layout.size(), size, 0);
                        self.info.deallocation(layout.size(), size, 0);
                    }
                }
            });
        } else {
            // No cache available, deallocate directly
            let mut allocator = self.fixed_size[index].lock();
            
            unsafe {
                allocator.deallocate(ptr, layout);
            }
            
            #[cfg(feature = "allocator-statistics")]
            {
                let size = Self::get_size(index);
                allocator.info_mut().deallocation(layout.size(), size, 0);
                self.info.deallocation(layout.size(), size, 0);
            }
        }
    }

    /// Проверяет, что `layout` поддерживается.
    ///
    /// # Panics
    ///
    /// Паникует, если `layout` не поддерживается.
    fn validate_layout(layout: Layout) {
        debug_assert_ne!(
            layout.size(),
            0,
            "can not handle zero-sized types requested by {layout:?}",
        );
    }

    /// Обновляет статистики [`Dispatcher::info`] и [`Dispatcher::fallback_info`],
    /// записывая в них операцию `operation` с заданным `layout`,
    /// которая была обслужена аллокатором [`Dispatcher::fallback`].
    fn update_fallback_info(
        &self,
        operation: Operation,
        layout: Layout,
    ) {
        let allocated_pages = layout.size().div_ceil(Page::SIZE);
        let allocated = allocated_pages * Page::SIZE;

        for info in [&self.fallback_info, &self.info] {
            match operation {
                Operation::Allocation => {
                    info.allocation(layout.size(), allocated, allocated_pages);
                },
                Operation::Deallocation => {
                    info.deallocation(layout.size(), allocated, allocated_pages);
                },
            }
        }
    }
}

unsafe impl<Q: Cache, T: Allocator + BigAllocatorGuard> GlobalAlloc for Dispatcher<Q, T> {
    // ANCHOR: alloc
    unsafe fn alloc(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        // ANCHOR_END: alloc
        Self::validate_layout(layout);

        if let Some(index) = self.get_fixed_size_index(layout) {
            self.fixed_size_allocate(index, layout)
        } else {
            self.fallback_allocate(layout, Initialize::Garbage)
        }
    }

    // ANCHOR: dealloc
    unsafe fn dealloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
    ) {
        // ANCHOR_END: dealloc
        Self::validate_layout(layout);

        if let Some(index) = self.get_fixed_size_index(layout) {
            unsafe {
                self.fixed_size_deallocate(index, ptr, layout);
            }
        } else {
            let ptr = NonNull::new(ptr).expect("should not get null ptr in dealloc()");

            unsafe {
                self.fallback.deallocate(ptr, layout);
            }

            self.update_fallback_info(Operation::Deallocation, layout);
        }
    }

    // ANCHOR: alloc_zeroed
    unsafe fn alloc_zeroed(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        // ANCHOR_END: alloc_zeroed
        Self::validate_layout(layout);

        if let Some(index) = self.get_fixed_size_index(layout) {
            let ptr = self.fixed_size_allocate(index, layout);
            if !ptr.is_null() {
                let size = Self::get_size(index);
                unsafe {
                    ptr::write_bytes(ptr, 0, size);
                }
            }
            ptr
        } else {
            self.fallback_allocate(layout, Initialize::Zero)
        }
    }

    // ANCHOR: realloc
    unsafe fn realloc(
        &self,
        old_ptr: *mut u8,
        old_layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        // ANCHOR_END: realloc
        let new_layout = Layout::from_size_align(new_size, old_layout.align())
            .expect("bad `old_layout` or `new_size` for realloc");

        Self::validate_layout(new_layout);

        let old_index = self.get_fixed_size_index(old_layout);
        let new_index = self.get_fixed_size_index(new_layout);

        match (old_index, new_index) {
            (Some(old_idx), Some(new_idx)) if old_idx == new_idx => {
                // Same size class, just update statistics
                #[cfg(feature = "allocator-statistics")]
                {
                    let size = Self::get_size(old_idx);
                    let mut allocator = self.fixed_size[old_idx].lock();
                    allocator.info_mut().deallocation(old_layout.size(), size, 0);
                    allocator.info_mut().allocation(new_layout.size(), size, 0);
                    self.info.deallocation(old_layout.size(), size, 0);
                    self.info.allocation(new_layout.size(), size, 0);
                }
                old_ptr
            },
            (Some(old_idx), Some(new_idx)) => {
                let new_ptr = self.fixed_size_allocate(new_idx, new_layout);
                if !new_ptr.is_null() {
                    let copy_size = cmp::min(old_layout.size(), new_layout.size());
                    unsafe {
                        ptr::copy_nonoverlapping(old_ptr, new_ptr, copy_size);
                    }
                    unsafe {
                        self.fixed_size_deallocate(old_idx, old_ptr, old_layout);
                    }
                }
                new_ptr
            },
            (Some(old_idx), None) => {
                let new_ptr = self.fallback_allocate(new_layout, Initialize::Garbage);
                if !new_ptr.is_null() {
                    let copy_size = cmp::min(old_layout.size(), new_layout.size());
                    unsafe {
                        ptr::copy_nonoverlapping(old_ptr, new_ptr, copy_size);
                    }
                    unsafe {
                        self.fixed_size_deallocate(old_idx, old_ptr, old_layout);
                    }
                }
                new_ptr
            },
            (None, Some(new_idx)) => {
                let new_ptr = self.fixed_size_allocate(new_idx, new_layout);
                if !new_ptr.is_null() {
                    let copy_size = cmp::min(old_layout.size(), new_layout.size());
                    unsafe {
                        ptr::copy_nonoverlapping(old_ptr, new_ptr, copy_size);
                    }
                    let old_ptr_nn = NonNull::new(old_ptr).expect("should not get null ptr in realloc()");
                    unsafe {
                        self.fallback.deallocate(old_ptr_nn, old_layout);
                    }
                    self.update_fallback_info(Operation::Deallocation, old_layout);
                }
                new_ptr
            },
            (None, None) => {
                let old_ptr = NonNull::new(old_ptr).expect("should not get null ptr in realloc()");

                let new_ptr = if old_layout.size() < new_layout.size() {
                    unsafe { self.fallback.grow(old_ptr, old_layout, new_layout) }
                } else {
                    unsafe { self.fallback.shrink(old_ptr, old_layout, new_layout) }
                };

                if let Ok(new_ptr) = new_ptr {
                    self.update_fallback_info(Operation::Allocation, new_layout);
                    self.update_fallback_info(Operation::Deallocation, old_layout);

                    new_ptr.as_non_null_ptr().as_ptr()
                } else {
                    ptr::null_mut()
                }
            },
        }
    }
}

impl<Q: Cache, T: Allocator + BigAllocatorGuard> Drop for Dispatcher<Q, T> {
    fn drop(&mut self) {
        self.unmap();
    }
}

/// Детальная статистика аллокатора [`Dispatcher`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetailedInfo {
    /// Общая статистика аллокатора [`Dispatcher`].
    total: Info,

    /// Статистика выделений и освобождений через [`Dispatcher::fallback`].
    fallback: Info,

    /// Статистика аллокаторов [`FixedSizeAllocator`] для разных размеров блоков.
    fixed_size: [Info; FIXED_SIZE_COUNT],
}

#[allow(rustdoc::private_intra_doc_links)]
impl DetailedInfo {
    /// Инициализирует детальную статистику аллокатора [`Dispatcher`] нулями.
    pub const fn new() -> Self {
        Self {
            total: Info::new(),
            fallback: Info::new(),
            fixed_size: [Info::new(); FIXED_SIZE_COUNT],
        }
    }

    /// Общая статистика аллокатора [`Dispatcher`].
    pub fn total(&self) -> &Info {
        &self.total
    }

    /// Статистика выделений и освобождений через [`Dispatcher::fallback`].
    pub fn fallback(&self) -> &Info {
        &self.fallback
    }

    /// Статистика аллокаторов [`FixedSizeAllocator`] для разных размеров блоков.
    pub fn fixed_size(&self) -> &[Info] {
        &self.fixed_size
    }

    /// Проверяет инварианты статистики аллокатора.
    ///
    /// Требует эксклюзивного доступа к аллокатору в момент снятия детальной статистики.
    /// Иначе из-за конкурентных операций статистики разойдутся.
    pub fn is_valid(&self) -> bool {
        if !self.total.is_valid() || !self.fallback.is_valid() {
            return false;
        }

        if let Ok(mut balance) = self.total - self.fallback {
            // `.iter()` is required to avoid copying `self.fixed_size`
            // which is too large to fit on the stack.
            // See also:
            //   - <https://github.com/rust-lang/rust/issues/45683>
            //   - <https://crates.io/crates/cargo-call-stack>
            for fixed_size in self.fixed_size.iter() {
                if !fixed_size.is_valid() {
                    return false;
                }

                if let Ok(new_balance) = balance - *fixed_size {
                    balance = new_balance;
                } else {
                    return false;
                }
            }

            balance == Info::new()
        } else {
            false
        }
    }
}

impl Default for DetailedInfo {
    fn default() -> Self {
        Self::new()
    }
}

// Avoid changing the info during formatting it due to requests to the global allocator.
impl fmt::Display for DetailedInfo {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let separator = if formatter.alternate() {
            "\n    "
        } else {
            " "
        };

        write!(
            formatter,
            "{{{0}valid: {1},{0}total: {2},{0}fallback: {3}",
            separator,
            self.is_valid(),
            self.total,
            self.fallback,
        )?;

        for (index, fixed_size) in self.fixed_size.iter().enumerate() {
            if fixed_size.allocations().positive() > 0 {
                let size = get_size(index);
                write!(formatter, ",{separator}size_{size}: {fixed_size}")?;
            }
        }

        if formatter.alternate() && self.total.allocated().positive() > 0 {
            assert!(self.total.allocations().positive() > 0);
            assert!(self.total.pages().positive() > 0);

            let total_allocations = self.total.allocations().positive() as f64;
            let total_allocated = self.total.allocated().positive() as f64;
            let total_pages = self.total.pages().positive() as f64;

            let format_ratio = |formatter: &mut fmt::Formatter, info: &Info| {
                write!(
                    formatter,
                    "{{ allocations: {:.3}%, allocated: {:.3}%, pages: {:.3}% }}",
                    info.allocations().positive() as f64 / total_allocations * 100.0,
                    info.allocated().positive() as f64 / total_allocated * 100.0,
                    info.pages().positive() as f64 / total_pages * 100.0,
                )
            };

            write!(
                formatter,
                ",{separator}allotment: {{{separator}    fallback: ",
            )?;
            format_ratio(formatter, &self.fallback)?;

            for (index, fixed_size) in self.fixed_size.iter().enumerate() {
                if fixed_size.allocations().positive() > 0 {
                    let size = get_size(index);
                    write!(formatter, ",{separator}    size_{size}: ")?;
                    format_ratio(formatter, fixed_size)?;
                }
            }

            write!(formatter, ",{separator}}}")?;
        }

        let separator = if formatter.alternate() {
            ",\n"
        } else {
            " "
        };

        write!(formatter, "{separator}}}")
    }
}

/// Операция, которая была выполнена.
#[derive(Debug)]
enum Operation {
    /// Выделение памяти.
    Allocation,

    /// Освобождение памяти.
    Deallocation,
}

/// Возвращает размер блоков памяти, за которые отвечает аллокатор
/// по индексу `index` в массиве [`Dispatcher::fixed_size`].
#[inline(always)]
fn get_size(index: usize) -> usize {
    (index + 1) * MIN_SIZE
}

#[allow(rustdoc::private_intra_doc_links)]
/// Количество аллокаторов фиксированного размера [`FixedSizeAllocator`],
/// которыми управляет [`Dispatcher`].
pub const FIXED_SIZE_COUNT: usize = (4 * Page::SIZE - 1) / MIN_SIZE;

/// Вспомогательная константа для инициализации массива [`Dispatcher::fixed_size`]
/// в константной функции [`Dispatcher::new()`].
/// Необходима, так как [`Spinlock`] не может быть помечен типажом [`Copy`].
#[allow(clippy::declare_interior_mutable_const)]
const FIXED_SIZE_DUMMY: Spinlock<FixedSizeAllocator> = Spinlock::new(FixedSizeAllocator::new());
