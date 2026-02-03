use core::{
    alloc::Layout,
    ops::Range,
};

use itertools::Itertools;

use crate::{
    error::{
        Error::{
            InvalidArgument,
            NoPage,
        },
        Result,
    },
    log::{
        error,
        info,
        trace,
    },
};

use super::{
    PAGES_PER_ROOT_LEVEL_ENTRY,
    addr::Virt,
    block::Block,
    frage::Page,
    mmu::PageTable,
    size::SizeOf,
    stack::Stack,
};

// Used in docs.
#[allow(unused)]
use crate::error::Error;

/// Простой аллокатор виртуальных страниц адресного пространства.
#[derive(Debug, Default)]
pub(super) struct PageAllocator {
    /// Блок свободных виртуальных страниц.
    block: Block<Page>,

    /// Первоначальное состояние блока [`PageAllocator::block`], сразу после отведения
    /// защитной зоны и до первого выделения виртуальных страниц вызывающему коду.
    /// Используется при дублировании аллокатора в методе [`PageAllocator::duplicate()`].
    initial_block: Block<Page>,
}

impl PageAllocator {
    /// Инициализирует аллокатор [`PageAllocator`]
    /// большим блоком подряд идущих свободных виртуальных страниц
    /// по информации из корневого узла таблицы страниц `page_table_root`.
    /// При этом `allowed_elements` задаёт диапазон записей корневого узла таблицы страниц,
    /// который можно использовать данному аллокатору.
    pub(super) fn new(
        page_table_root: &PageTable,
        allowed_elements: Range<usize>,
    ) -> Self {
        if let Some(block) = Self::find_unused_block(page_table_root, allowed_elements) {
            let mut page_allocator = Self {
                block,
                initial_block: Block::default(),
            };

            // Detect stack overruns that corrupt memory allocated by PageAllocator.
            let message = "failed to create guard zone for PageAllocator";
            page_allocator
                .allocate(Page::layout(Stack::GUARD_ZONE_SIZE).expect(message))
                .expect(message);

            page_allocator.initial_block = page_allocator.block;

            info!(
                free_page_count = page_allocator.block.count(),
                block = %page_allocator.block,
                "page allocator init",
            );

            page_allocator
        } else {
            error!(?page_table_root);
            panic!(
                "failed to find a free entry in the page table root for the virtual page allocator",
            );
        }
    }

    /// Возвращает пустой [`PageAllocator`].
    /// В отличие от [`PageAllocator::default()`] доступна в константном контексте.
    pub(super) const fn zero() -> Self {
        Self {
            block: Block::zero(),
            initial_block: Block::zero(),
        }
    }

    /// Создаёт копию [`PageAllocator`] на момент его инициализации.
    pub(super) fn duplicate(&self) -> Self {
        Self {
            block: self.initial_block,
            initial_block: self.initial_block,
        }
    }

    /// Устанавливает [`PageAllocator`] в состояние эквивалентное `original`.
    /// Текущий [`PageAllocator`] должен был быть получен
    /// из `original` методом [`PageAllocator::duplicate()`].
    /// И из него не должно было быть выделено больше страниц, чем из `original`.
    /// Если это не так, возвращается ошибка [`Error::InvalidArgument`].
    pub(super) fn duplicate_allocator_state(
        &mut self,
        original: &Self,
    ) -> Result<()> {
        if self.block.contains_block(original.block) {
            self.block = original.block;
            Ok(())
        } else {
            Err(InvalidArgument)
        }
    }

    // ANCHOR: allocate
    /// Выделяет блок подряд идущих виртуальных страниц для хранения объекта,
    /// требования к размещению в памяти которого описывает `layout`.
    /// Ни выделения физической памяти, ни создания отображения станиц, не происходит.
    ///
    /// Если выделить заданный размер виртуальной памяти не удалось,
    /// возвращает ошибку [`Error::NoPage`].
    pub(super) fn allocate(
        &mut self,
        layout: Layout,
    ) -> Result<Block<Page>> {
        // ANCHOR_END: allocate
        let pages_needed = Page::count_up(layout.size());
        if pages_needed == 0 {
            return Ok(Block::zero());
        }
        let alignment = layout.align();
        let extra_pages_for_alignment = if alignment > Page::SIZE {
            alignment.div_ceil(Page::SIZE)
        } else {
            0
        };
        
        let total_pages_needed = pages_needed + extra_pages_for_alignment;
        if total_pages_needed > self.block.count() {
            return Err(NoPage);
        }
        let mut allocated_block = if let Some(block) = self.block.tail(total_pages_needed) {
            block
        } else {
            return Err(NoPage);
        };
        if alignment > Page::SIZE {
            let start_address = allocated_block.start_address().into_usize();
            
            if !start_address.is_multiple_of(alignment) {
                let misalignment = start_address % alignment;
                let adjustment = alignment - misalignment;
                let pages_to_skip = adjustment.div_ceil(Page::SIZE);
                if pages_to_skip < allocated_block.count() {
                    let new_start_index = allocated_block.start() + pages_to_skip;
                    let new_end_index = allocated_block.end();
                    allocated_block = Block::from_index(new_start_index, new_end_index)
                        .map_err(|_| NoPage)?;
                    if allocated_block.count() < pages_needed {
                        return Err(NoPage);
                    }
                } else {
                    return Err(NoPage);
                }
            }
        }
        if allocated_block.count() > pages_needed {
            let final_end_index = allocated_block.start() + pages_needed;
            allocated_block = Block::from_index(allocated_block.start(), final_end_index)
                .map_err(|_| NoPage)?;
        }
        
        Ok(allocated_block)
    }

    /// Обратный метод к [`PageAllocator::allocate()`].
    /// Освобождает блок виртуальных страниц `block`.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidArgument`] --- заданный блок не является целиком выделенным.
    pub(super) fn deallocate(
        &mut self,
        pages: Block<Page>,
    ) -> Result<()> {
        if self.initial_block.contains_block(pages) && self.block.is_disjoint(pages) {
            Ok(())
        } else {
            Err(InvalidArgument)
        }
    }

    /// Отмечает заданный блок виртуальных страниц как используемый.
    /// Ни выделения физической памяти, ни создания отображения станиц, не происходит.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- заданный блок не является целиком свободным.
    pub(super) fn reserve(
        &mut self,
        pages: Block<Page>,
    ) -> Result<()> {
        if self.block.contains_block(pages) {
            self.block = if pages.start() - self.block.start() < self.block.end() - pages.end() {
                Block::from_index(pages.end(), self.block.end())
            } else {
                Block::from_index(self.block.start(), pages.start())
            }?;

            Ok(())
        } else {
            Err(NoPage)
        }
    }

    // ANCHOR: find_unused_block
    /// Возвращает блок виртуальных страниц,
    /// соответствующий самой длинной последовательности пустых записей в
    /// диапазоне `allowed_elements` корневого узла таблицы страниц `page_table_root`.
    ///
    /// Для упрощения не смотрит в другие узлы таблицы страниц,
    /// и резервирует только страницы, соответствующие полностью свободным
    /// записям таблицы страниц корневого уровня.
    ///
    /// Если все записи заняты, возвращает `None`.
    fn find_unused_block(
        page_table_root: &PageTable,
        allowed_elements: Range<usize>,
    ) -> Option<Block<Page>> {
        // ANCHOR_END: find_unused_block
        let mut longest_start = None;
        let mut longest_length = 0;
        
        let mut current_start = None;
        let mut current_length = 0;
        
        for i in allowed_elements {
            if !page_table_root[i].is_present() {
                if current_start.is_none() {
                    current_start = Some(i);
                    current_length = 1;
                } else {
                    current_length += 1;
                }
                if current_length > longest_length {
                    longest_start = current_start;
                    longest_length = current_length;
                }
            } else {
                current_start = None;
                current_length = 0;
            }
        }
        if let Some(start) = longest_start {
            let page_start = start * PAGES_PER_ROOT_LEVEL_ENTRY;
            let page_end = page_start + longest_length * PAGES_PER_ROOT_LEVEL_ENTRY;
            Block::from_index(page_start, page_end).ok()
        } else {
            None
        }
    }
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use core::ops::Range;

    use super::{
        super::{
            Block,
            Page,
            mmu::PageTable,
        },
        PageAllocator,
    };

    pub(in super::super) fn block(page_allocator: &PageAllocator) -> Block<Page> {
        page_allocator.block
    }

    pub fn find_unused_block(
        page_table_root: &PageTable,
        allowed_elements: Range<usize>,
    ) -> Option<Block<Page>> {
        PageAllocator::find_unused_block(page_table_root, allowed_elements)
    }
}
