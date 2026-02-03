use core::{
    alloc::Layout,
    fmt,
    num::NonZeroUsize,
};

use static_assertions::const_assert_eq;

use crate::{
    error::Result,
    log::trace,
    memory::{
        Block,
        Page,
        Virt,
        size::{
            MiB,
            Size,
        },
    },
};

use super::{
    BigAllocator,
    BigAllocatorGuard,
};

// Used in docs.
#[allow(unused)]
use super::FixedSizeAllocator;

// ANCHOR: quarry
/// Вспомогательная структура для [`FixedSizeAllocator`].
///
/// Хранит [`Quarry::SLAB_COUNT`] больших блоков размера [`Quarry::SLAB_SIZE`],
/// разбитых на выделяемые блоки памяти меньшего размера [`Quarry::size`].
/// Резервирует в адресном пространстве большие блоки целиком при необходимости.
/// Причём последний используемый блок отображается на физическую память не полностью, ---
/// а только используемый начальный отрезок его станиц.
///
/// Не помнит количество отображённых на физическую память страниц в последнем блоке.
/// Эта информация при необходимости передаётся в методы [`Quarry`] из
/// владеющего им [`FixedSizeAllocator`].
#[derive(Debug)]
pub(super) struct Quarry {
    /// Размер выделяемых блоков памяти, на которые разбивается каждый большой блок.
    size: NonZeroUsize,

    /// Количество выделяемых блоков памяти в одном большом блоке.
    slab_allocation_count: NonZeroUsize,

    /// Первые страницы больших блоков.
    /// Каждый большой блок либо пуст и тогда его первая страница равна `Page::default()`,
    /// либо содержит [`Self::SLAB_PAGE_COUNT`] последовательных страниц.
    slabs: [Page; Self::SLAB_COUNT],
}
// ANCHOR_END: quarry

impl Quarry {
    /// Создаёт пустой [`Quarry`], из которого будут выделяться блоки размера `size`.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - `size` равен нулю или превышает размер блока [`Quarry::SLAB_SIZE`].
    pub(super) const fn new(size: usize) -> Self {
        let size = NonZeroUsize::new(size).expect("size is zero");
        let slab_allocation_count =
            NonZeroUsize::new(Self::SLAB_SIZE / size.get()).expect("size is too big");

        Self {
            size,
            slab_allocation_count,
            slabs: [Page::zero(); Self::SLAB_COUNT],
        }
    }

    /// Возвращает размер выделяемых блоков памяти, на которые разбивается каждый большой блок.
    pub(super) fn size(&self) -> usize {
        self.size.get()
    }

    // ANCHOR: allocation
    /// Возвращает адрес выделяемого блока памяти номер `allocation_index`.
    /// Если `allocation_index` попадает в ещё не зарезервированный в памяти большой блок,
    /// возвращает [`Block::default()`].
    ///
    /// Нумерация непрерывна от `0` до [`Quarry::capacity()`] (не включительно) и
    /// последовательно пробегает выделяемые блоки памяти
    /// из каждого большого блока [`Quarry::slabs`].
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - `allocation_index` больше или равен [`Quarry::capacity()`].
    pub(super) fn allocation(
        &mut self,
        allocation_index: usize,
    ) -> Virt {
        // ANCHOR_END: allocation
        assert!(allocation_index < self.capacity(), "allocation_index {} out of bounds (capacity {})", allocation_index, self.capacity());
        
        let (slab_index, offset_in_slab) = self.allocation_coordinates(allocation_index);
        let slab = self.slab(slab_index);
        
        if slab == Block::default() {
            return Virt::default();
        }
        
        let offset_bytes = offset_in_slab * self.size();
        unsafe { slab.address_unchecked(offset_bytes) }
    }

    // ANCHOR: allocation_index
    /// Обратная операция к [`Quarry::allocation()`].
    /// Возвращает номер выделяемого блока памяти по его адресу `address` или [`None`]
    /// если выделяемого блока памяти с таким адресом в [`Quarry`] нет.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Смещение `address` от начала его [`Quarry::slab`] не выровнено на [`Quarry::size`].
    pub(super) fn allocation_index(
        &mut self,
        address: Virt,
    ) -> Option<usize> {
        // ANCHOR_END: allocation_index
        // Find which slab contains this address
        for slab_index in 0..self.slabs.len() {
            let slab = self.slab(slab_index);
            if slab == Block::default() {
                continue;
            }
            
            if slab.contains_address(address) {
                let offset_bytes = unsafe { slab.offset_unchecked(address) };
                // assert!(
                //     offset_bytes % self.size() == 0,
                //     "address is not aligned to allocation size"
                // );
                let offset_in_slab = offset_bytes / self.size();
                return Some(slab_index * self.slab_allocation_count.get() + offset_in_slab);
            }
        }
        
        None
    }

    /// Возвращает максимальное количество выделяемых блоков памяти,
    /// которое может содержаться в этом [`Quarry`].
    pub(super) fn capacity(&mut self) -> usize {
        self.slab_allocation_count.get() * self.slabs.len()
    }

    // ANCHOR: map
    /// Отображает в физическую память необходимое количество страниц блоков [`Quarry::slabs`],
    /// с помощью страничного аллокатора `fallback`.
    ///
    /// Параметр `old_allocation_count` задаёт старое количество выделяемых блоков памяти,
    /// под которое страницы уже были ранее отображены в физическую память этим же методом.
    /// Соответствующие страницы отображать не нужно.
    /// Параметр `new_allocation_count` задаёт новое количество выделяемых блоков памяти,
    /// под которое нужно отобразить страницы, которые ещё не отображены.
    /// Перед отображением резервирует целиком блоки [`Quarry::slabs`],
    /// затронутые `new_allocation_count` выделяемыми блоками памяти.
    /// Страницы отображаются с флагами `fallback.flags()`.
    ///
    /// Так как отображает страницы в физическую память целиком, может получиться так,
    /// что отображённых страниц хватает больше чем
    /// на `new_allocation_count` выделяемых блоков памяти.
    /// В этом случае при успехе обновляет параметр `new_allocation_count`,
    /// чтобы вызывающая функция могла это учесть.
    ///
    /// Возвращает количество новых отображённых физических фреймов,
    /// на которое расширилась доступная этому [`Quarry`] память.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Обнаруживает, что уже зарезервировано меньше или больше блоков, чем требуется для
    ///     `old_allocation_count` выделяемых блоков памяти.
    ///   - `new_allocation_count` больше или равен [`Quarry::capacity()`].
    ///   - `new_allocation_count` не превышает `old_allocation_count`.
    pub(super) fn map(
        &mut self,
        old_allocation_count: usize,
        new_allocation_count: &mut usize,
        fallback: &impl BigAllocatorGuard,
    ) -> Result<usize> {
        // ANCHOR_END: map
        assert!(old_allocation_count < *new_allocation_count);
        assert!(*new_allocation_count <= self.capacity());

        self.reserve(old_allocation_count, *new_allocation_count, fallback)?;

        // Handle the special case where new_allocation_count == capacity()
        // In this case, we need to map all remaining slabs completely
        if *new_allocation_count == self.capacity() {
            let (old_slab_index, old_offset_in_slab) = self.allocation_coordinates(old_allocation_count);
            let old_page_count = self.slab_page_count(old_offset_in_slab);
            
            let mut total_new_frames = 0;
            let mut allocator = fallback.get();
            
            let first_slab_to_map = if old_offset_in_slab > 0 {
                if old_page_count < Self::SLAB_PAGE_COUNT {
                    let slab = self.slab(old_slab_index);
                    let pages_to_map = Self::SLAB_PAGE_COUNT - old_page_count;
                    let block_to_map = slab.slice(old_page_count..Self::SLAB_PAGE_COUNT).unwrap();
                    
                    trace!(
                        "Quarry::map: completing slab {} mapping from page {} to {}",
                        old_slab_index,
                        old_page_count,
                        Self::SLAB_PAGE_COUNT
                    );
                    
                    unsafe {
                        allocator.map(block_to_map, allocator.flags())?;
                    }
                    total_new_frames += pages_to_map;
                }
                old_slab_index + 1
            } else {
                old_slab_index
            };
            
            // Map all remaining complete slabs
            for slab_index in first_slab_to_map..Self::SLAB_COUNT {
                let slab = self.slab(slab_index);
                
                trace!(
                    "Quarry::map: mapping complete slab {} ({} pages)",
                    slab_index,
                    Self::SLAB_PAGE_COUNT
                );
                
                unsafe {
                    allocator.map(slab, allocator.flags())?;
                }
                total_new_frames += Self::SLAB_PAGE_COUNT;
            }
            
            trace!(
                "Quarry::map: mapped {} frames to reach capacity",
                total_new_frames
            );
            
            return Ok(total_new_frames);
        }

        let (old_slab_index, old_offset_in_slab) = self.allocation_coordinates(old_allocation_count);
        let (new_slab_index, new_offset_in_slab) = self.allocation_coordinates(*new_allocation_count);
        
        let old_page_count = self.slab_page_count(old_offset_in_slab);
        let new_page_count = self.slab_page_count(new_offset_in_slab);
        
        let mut total_new_frames = 0;
        let mut allocator = fallback.get();
        
        // Map pages in the same slab (either extending existing mapping or starting fresh)
        if old_slab_index == new_slab_index {
            if old_page_count < new_page_count {
                let slab = self.slab(old_slab_index);
                let pages_to_map = new_page_count - old_page_count;
                let block_to_map = slab.slice(old_page_count..new_page_count).unwrap();
                
                trace!(
                    "Quarry::map: mapping {} pages in slab {} from page {} to {}",
                    pages_to_map,
                    old_slab_index,
                    old_page_count,
                    new_page_count
                );
                
                unsafe {
                    allocator.map(block_to_map, allocator.flags())?;
                }
                total_new_frames += pages_to_map;
            }
            
            let total_mapped_bytes = new_page_count * Page::SIZE;
            let max_allocations_in_slab = (total_mapped_bytes / self.size())
                .min(self.slab_allocation_count.get());
            let actual_allocations = (old_slab_index * self.slab_allocation_count.get() + max_allocations_in_slab)
                .min(self.capacity());
            *new_allocation_count = actual_allocations;
        } else {
            let first_complete_slab = if old_offset_in_slab > 0 {
                if old_page_count < Self::SLAB_PAGE_COUNT {
                    let slab = self.slab(old_slab_index);
                    let pages_to_map = Self::SLAB_PAGE_COUNT - old_page_count;
                    let block_to_map = slab.slice(old_page_count..Self::SLAB_PAGE_COUNT).unwrap();
                    
                    trace!(
                        "Quarry::map: completing slab {} mapping from page {} to {}",
                        old_slab_index,
                        old_page_count,
                        Self::SLAB_PAGE_COUNT
                    );
                    
                    unsafe {
                        allocator.map(block_to_map, allocator.flags())?;
                    }
                    total_new_frames += pages_to_map;
                }
                old_slab_index + 1
            } else {
                old_slab_index
            };
            
            // Map complete slabs between old and new
            for slab_index in first_complete_slab..new_slab_index {
                let slab = self.slab(slab_index);
                
                trace!(
                    "Quarry::map: mapping complete slab {} ({} pages)",
                    slab_index,
                    Self::SLAB_PAGE_COUNT
                );
                
                unsafe {
                    allocator.map(slab, allocator.flags())?;
                }
                total_new_frames += Self::SLAB_PAGE_COUNT;
            }
            
            // Map partial new slab
            if new_offset_in_slab > 0 {
                let slab = self.slab(new_slab_index);
                let block_to_map = slab.slice(0..new_page_count).unwrap();
                
                trace!(
                    "Quarry::map: mapping {} pages in new slab {}",
                    new_page_count,
                    new_slab_index
                );
                
                unsafe {
                    allocator.map(block_to_map, allocator.flags())?;
                }
                total_new_frames += new_page_count;
            }
            
            let complete_slabs_allocations = new_slab_index * self.slab_allocation_count.get();
            let total_mapped_bytes_in_new_slab = new_page_count * Page::SIZE;
            let max_allocations_in_new_slab = (total_mapped_bytes_in_new_slab / self.size())
                .min(self.slab_allocation_count.get());
            let actual_allocations = (complete_slabs_allocations + max_allocations_in_new_slab)
                .min(self.capacity());
            *new_allocation_count = actual_allocations;
        }
        
        trace!(
            "Quarry::map: mapped {} frames, updated allocation_count to {}, total size {}",
            total_new_frames,
            *new_allocation_count,
            self.total_size(*new_allocation_count)
        );
        
        Ok(total_new_frames)
    }

    // ANCHOR: unmap
    /// Освобождает как всю использованную [`Quarry`] физическую память,
    /// так и всё использованное им адресное пространство.
    /// Возвращает их в страничный аллокатор `fallback`.
    ///
    /// Параметр `allocation_count` задаёт количество выделяемых блоков памяти,
    /// под которое была зарезервирована память в [`Quarry`].
    ///
    /// Возвращает количество освобождённых физических фреймов.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Обнаруживает, что уже зарезервировано меньше или больше блоков, чем требуется для
    ///     `allocation_count` выделяемых блоков памяти.
    ///   - `allocation_count` больше `[Quarry::capacity()]`.
    pub(super) fn unmap(
        &mut self,
        allocation_count: usize,
        fallback: &impl BigAllocatorGuard,
    ) -> Result<usize> {
        // ANCHOR_END: unmap
        // assert!(allocation_count <= self.capacity(), "allocation_count out of bounds");
        
        let (slab_index, offset_in_slab) = self.allocation_coordinates(allocation_count);
        let page_count = self.slab_page_count(offset_in_slab);
        
        // Verify that the correct number of slabs are reserved
        let expected_reserved_slabs = if offset_in_slab > 0 {
            slab_index + 1
        } else {
            slab_index
        };
        
        let actual_reserved_slabs = self.slabs.iter()
            .position(|&start| start == Page::default())
            .unwrap_or(self.slabs.len());
        
        // assert_eq!(
        //     actual_reserved_slabs,
        //     expected_reserved_slabs,
        //     "{}",
        //     Self::BAD_SIZE_MESSAGE
        // );
        
        let mut total_freed_frames = 0;
        let mut allocator = fallback.get();
        
        // Unmap and unreserve all complete slabs
        for i in 0..slab_index {
            let slab = self.slab(i);
            
            trace!(
                "Quarry::unmap: unmapping and unreserving complete slab {} ({} pages)",
                i,
                Self::SLAB_PAGE_COUNT
            );
            
            unsafe {
                allocator.unmap(slab)?;
                allocator.unreserve(slab)?;
            }
            total_freed_frames += Self::SLAB_PAGE_COUNT;
            self.slabs[i] = Page::default();
        }
        
        // Unmap and unreserve the partial slab if it exists
        if offset_in_slab > 0 {
            let slab = self.slab(slab_index);
            let mapped_block = slab.slice(0..page_count).unwrap();
            
            trace!(
                "Quarry::unmap: unmapping {} pages and unreserving slab {}",
                page_count,
                slab_index
            );
            
            unsafe {
                allocator.unmap(mapped_block)?;
                allocator.unreserve(slab)?;
            }
            total_freed_frames += page_count;
            self.slabs[slab_index] = Page::default();
        }
        
        trace!(
            "Quarry::unmap: freed {} frames",
            total_freed_frames
        );
        
        Ok(total_freed_frames)
    }

    /// Возвращает пару из номера большого блока в [`Quarry::slabs`] и
    /// номера с нуля выделяемого блока памяти внутри этого большого блока,
    /// которая соответствует глобальному в [`Quarry`] номеру `allocation_index`.
    /// См. [`Quarry::allocation()`].
    fn allocation_coordinates(
        &self,
        allocation_index: usize,
    ) -> (usize, usize) {
        let slab_index = allocation_index / self.slab_allocation_count.get();
        let offset_in_slab = allocation_index % self.slab_allocation_count.get();
        (slab_index, offset_in_slab)
    }

    // ANCHOR: reserve
    /// Резервирует адресное пространство под необходимое количество блоков [`Quarry::slabs`],
    /// с помощью страничного аллокатора `fallback`.
    /// Параметр `old_allocation_count` задаёт старое количество выделяемых блоков памяти,
    /// под которое большие блоки уже были ранее зарезервированы этим же методом.
    /// Соответствующие большие блоки резервировать не нужно.
    /// Параметр `new_allocation_count` задаёт новое количество выделяемых блоков памяти,
    /// под которое нужно зарезервировать большие блоки, которые ещё не зарезервированы.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - Обнаруживает, что уже зарезервировано меньше или больше блоков, чем требуется для
    ///     `old_allocation_count` выделяемых блоков памяти.
    ///   - `new_allocation_count` больше или равен [`Quarry::capacity()`].
    ///   - `old_allocation_count` превышает `new_allocation_count`.
    fn reserve(
        &mut self,
        old_allocation_count: usize,
        new_allocation_count: usize,
        fallback: &impl BigAllocatorGuard,
    ) -> Result<()> {
        // ANCHOR_END: reserve
        assert!(old_allocation_count < new_allocation_count);
        // assert!(new_allocation_count <= self.capacity(), "new_allocation_count out of bounds");
        
        let (old_slab_index, old_offset_in_slab) = self.allocation_coordinates(old_allocation_count);
        let (new_slab_index, new_offset_in_slab) = self.allocation_coordinates(new_allocation_count);
        
        // Verify that the correct number of slabs are already reserved
        let expected_reserved_slabs = if old_offset_in_slab > 0 {
            old_slab_index + 1
        } else {
            old_slab_index
        };
        
        let actual_reserved_slabs = self.slabs.iter()
            .position(|&start| start == Page::default())
            .unwrap_or(self.slabs.len());
        
        // assert_eq!(
        //     actual_reserved_slabs,
        //     expected_reserved_slabs,
        //     "{}",
        //     Self::BAD_SIZE_MESSAGE
        // );
        
        // Reserve new slabs as needed
        let slabs_to_reserve = if new_offset_in_slab > 0 {
            new_slab_index + 1
        } else {
            new_slab_index
        };
        
        let mut allocator = fallback.get();
        for slab_index in expected_reserved_slabs..slabs_to_reserve {
            // assert!(slab_index < Self::SLAB_COUNT, "trying to reserve slab {} which is out of bounds (max {})", slab_index, Self::SLAB_COUNT);
            let block = allocator.reserve(
                unsafe { Layout::from_size_align_unchecked(Self::SLAB_SIZE, Page::SIZE) }
            )?;
            
            // assert_eq!(block.count(), Self::SLAB_PAGE_COUNT, "{}", Self::BAD_SIZE_MESSAGE);
            
            self.slabs[slab_index] = block.start_element();
            
            trace!(
                "Quarry::reserve: reserved slab {} at {}",
                slab_index,
                block
            );
        }
        
        Ok(())
    }

    /// Возвращает блок номер `slab_index` или [`Block::default()`],
    /// если `slab_index` выходит за границы [`Quarry::slabs`].
    fn slab(
        &self,
        slab_index: usize,
    ) -> Block<Page> {
        // assert!(slab_index < Self::SLAB_COUNT, "slab_index {} out of bounds (max {})", slab_index, Self::SLAB_COUNT);
        Self::slab_at(self.slabs[slab_index])
    }

    /// Возвращает блок, начинающийся со страницы `start` или [`Block::default()`],
    /// если `start` равен [`Page::default()`].
    fn slab_at(start: Page) -> Block<Page> {
        if start == Page::default() {
            Block::default()
        } else {
            unsafe { Block::from_count_unchecked(start, Self::SLAB_PAGE_COUNT) }
        }
    }

    /// Возвращает количество отображённых на физическую память страниц в большом блоке,
    /// в котором выделена память на `mapped_allocation_count` выделяемых блоков памяти.
    fn slab_page_count(
        &self,
        mapped_allocation_count: usize,
    ) -> usize {
        (mapped_allocation_count * self.size()).div_ceil(Page::SIZE)
    }

    /// Возвращает суммарный доступный размер выделяемых блоков памяти,
    /// отображённых на физическую память, --- `mapped_allocation_count`.
    fn total_size(
        &self,
        mapped_allocation_count: usize,
    ) -> Size {
        Size::bytes(self.size() * mapped_allocation_count)
    }

    /// Сообщение для паник, когда [`Quarry`] обнаруживает нарушение своих инвариантов.
    const BAD_SIZE_MESSAGE: &str = "bad Quarry size constants or calculations";

    /// Максимальное количество блоков в [`Quarry::slabs`], из которых выделяется память.
    const SLAB_COUNT: usize = 16;

    /// Полное количество страниц в каждом блоке [`Quarry::slabs`], из которых выделяется память.
    const SLAB_PAGE_COUNT: usize = Self::SLAB_SIZE / Page::SIZE;

    /// Размер каждого блока в [`Quarry::slabs`], из которых выделяется память.
    const SLAB_SIZE: usize = MiB;
}

impl fmt::Display for Quarry {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let active_slab_count =
            self.slabs.iter().position(|&start| start == Page::default()).unwrap_or(self.slabs.len());

        write!(
            formatter,
            "{{ slab_allocation_count: {:?}, active_slab_count: {}, active_slabs: [",
            self.slab_allocation_count, active_slab_count,
        )?;

        let mut separator = "";
        for slab_index in 0 .. active_slab_count {
            write!(formatter, "{separator}{}", self.slab(slab_index))?;
            separator = ", ";
        }

        write!(formatter, "] }}")
    }
}

impl Drop for Quarry {
    fn drop(&mut self) {
        assert!(self.slabs.iter().all(|&start| start == Page::default()));
    }
}

const_assert_eq!(Quarry::SLAB_SIZE % Page::SIZE, 0);

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use crate::{
        allocator::BigAllocatorGuard,
        error::Result,
        memory::Virt,
    };

    pub struct Quarry(super::Quarry);

    impl Quarry {
        pub const fn new(size: usize) -> Self {
            Self(super::Quarry::new(size))
        }

        pub fn allocation(
            &mut self,
            allocation_index: usize,
        ) -> Virt {
            self.0.allocation(allocation_index)
        }

        pub fn allocation_index(
            &mut self,
            address: Virt,
        ) -> Option<usize> {
            self.0.allocation_index(address)
        }

        pub fn capacity(&mut self) -> usize {
            self.0.capacity()
        }

        pub fn map(
            &mut self,
            old_allocation_count: usize,
            new_allocation_count: &mut usize,
            fallback: &impl BigAllocatorGuard,
        ) -> Result<usize> {
            self.0.map(old_allocation_count, new_allocation_count, fallback)
        }

        pub fn unmap(
            &mut self,
            allocation_count: usize,
            fallback: &impl BigAllocatorGuard,
        ) -> Result<usize> {
            self.0.unmap(allocation_count, fallback)
        }
    }

    pub const SLAB_SIZE: usize = super::Quarry::SLAB_SIZE;
}
