use core::{
    fmt,
    ptr::NonNull,
};

use alloc::vec::Vec;

use duplicate::duplicate_item;

use crate::{
    error::{
        Error::{
            NoPage,
            Unimplemented,
        },
        Result,
    },
    log::trace,
};

use super::{
    Block,
    FULL_ACCESS,
    FrameGuard,
    Mapping,
    Virt,
    frage::{
        L1_SIZE,
        L2_SIZE,
        Page,
    },
    mapped_block::MappedBlock,
    mmu::{
        self,
        PAGE_TABLE_ENTRY_COUNT,
        PAGE_TABLE_LEAF_LEVEL,
        PAGE_TABLE_LEVEL_COUNT,
        PAGE_TABLE_ROOT_LEVEL,
        PageTableEntry,
        PageTableFlags,
    },
    size::{
        self,
        Size,
    },
};

// Used in docs.
#[allow(unused)]
use {
    super::FRAME_ALLOCATOR,
    ku::error::Error,
};

/// Путь в дереве отображения заданного виртуального адреса.
#[derive(Debug, Eq, PartialEq)]
pub struct Path<'a> {
    /// Дерево отображения.
    mapping: &'a mut Mapping,

    /// Узлы на пути в дереве отображения,
    /// задаваемые как указатели на соответствующие [`PageTableEntry`].
    /// Элемент с индексом [`PAGE_TABLE_LEAF_LEVEL`] задаёт указатель на [`PageTableEntry`],
    /// отображающей виртуальную страницу адреса [`Path::virt`] на его физическую страницу.
    /// Элемент с индексом [`PAGE_TABLE_ROOT_LEVEL`] задаёт указатель на [`PageTableEntry`],
    /// находящуюся в корневой таблице страниц.
    /// Если какой-то из узлов не отображён в память,
    /// соответствующий элемент равен [`None`].
    nodes: [Option<NonNull<PageTableEntry>>; PAGE_TABLE_LEVEL_COUNT],

    /// Адрес, для которого построен путь отображения.
    virt: Virt,
}

impl<'a> Path<'a> {
    /// Создаёт путь в дереве отображения заданного виртуального адреса.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    /// - На корневом уровне [`PAGE_TABLE_ROOT_LEVEL`] нет [`PageTableEntry`].
    /// - Выше [`Path::deepest_pte()`] есть [`PageTableEntry`] для которой
    ///   [`PageTableEntry::is_present()`] возвращает `false`.
    /// - Глубже [`Path::deepest_pte()`] есть существующие [`PageTableEntry`].
    /// - Если самая глубокая [`PageTableEntry`] расположена выше [`PAGE_TABLE_LEAF_LEVEL`]
    ///   и при этом для неё [`PageTableEntry::is_present()`] и [`PageTableEntry::is_huge()`]
    ///   возвращают разные значения.
    pub(super) fn new(
        mapping: &'a mut Mapping,
        nodes: [Option<NonNull<PageTableEntry>>; PAGE_TABLE_LEVEL_COUNT],
        virt: Virt,
    ) -> Self {
        let path = Self {
            mapping,
            nodes,
            virt,
        };

        path.validate();

        path
    }

    /// Возвращает отображение блока виртуальных страниц
    /// на блок физических фреймов, если текущий [`Path`]
    /// задаёт путь к листу дерева отображения.
    pub fn block(&self) -> MappedBlock {
        let (level, pte) = self.deepest_pte();

        let flags = pte.flags();
        let frame = if pte.is_huge() {
            pte.huge_frame().expect(Self::INVALID_PATH)
        } else {
            pte.frame().unwrap_or_default()
        };

        let pages = self.pages(level);
        let frames = Block::from_index(frame.index(), frame.index() + pages.count())
            .expect(Self::INVALID_PATH);

        MappedBlock::new(flags, frames, pages)
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Возвращает листьевую [`PageTableEntry`], которая отвечает за отображение адреса [`Path::virt`].
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- промежуточного или нужного листьевого узла таблицы страниц нет.
    /// - [`Error::Unimplemented`] --- промежуточный узел таблицы страниц
    ///   имеет флаг [`PageTableFlags::HUGE`].
    #[allow(clippy::needless_arbitrary_self_type)]
    #[duplicate_item(
        getter self_type return_type deepest_pte_getter;
        [get] [&Self] [&'a PageTableEntry] [deepest_pte];
        [get_mut] [&mut Self] [&'a mut PageTableEntry] [deepest_pte_mut];
    )]
    pub(super) fn getter(self: self_type) -> Result<return_type> {
        let (level, pte) = self.deepest_pte_getter();

        if level == PAGE_TABLE_LEAF_LEVEL {
            Ok(pte)
        } else if pte.is_huge() {
            Err(Unimplemented)
        } else {
            Err(NoPage)
        }
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Выделяет физические фреймы под отсутствующие промежуточные таблицы страничного отображения.
    /// И исправляет флаги с которыми они отображены так, чтобы целевой адрес [`Path::virt`]
    /// можно было отобразить с эффективными флагами `flags`.
    /// Отображает заданный `frame` с флагами `flags`.
    /// Освобождает фрейм, который был ранее отображён, если он есть.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::NoFrame`] если пришлось выделить физический фрейм,
    ///     но их не осталось во [`static@FRAME_ALLOCATOR`].
    ///   - [`Error::Unimplemented`] если промежуточный узел таблицы страниц
    ///     имеет флаг [`PageTableFlags::HUGE`].
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в удаляемую страницу.
    pub unsafe fn map(
        &mut self,
        frame: FrameGuard,
        flags: PageTableFlags,
    ) -> Result<()> {
        let (deepest_level, deepest_pte) = self.deepest_pte_mut();
        if deepest_pte.is_huge() {
            return Err(Unimplemented);
        }
        let mut current_frame = self.mapping.page_table_root();
        for level in (PAGE_TABLE_LEAF_LEVEL + 1..=PAGE_TABLE_ROOT_LEVEL).rev() {
            let (is_present, is_huge) = {
                let pte = unsafe { self.mapping.pte_mut(self.virt, level, current_frame) };
                (pte.is_present(), pte.is_huge())
            };
            
            if !is_present {
                let new_frame_guard = self.mapping.allocate_node()?;
                let new_frame = *new_frame_guard;
                let pte = unsafe { self.mapping.pte_mut(self.virt, level, current_frame) };
                let intermediate_flags = (flags & FULL_ACCESS) | PageTableFlags::PRESENT;
                new_frame_guard.store(pte, intermediate_flags);
                current_frame = new_frame;
            } else {
                if is_huge {
                    return Err(Unimplemented);
                }
                let pte = unsafe { self.mapping.pte_mut(self.virt, level, current_frame) };
                current_frame = pte.frame().map_err(|_| NoPage)?;
                let required_flags = (flags & FULL_ACCESS) | PageTableFlags::PRESENT;
                let current_flags = pte.flags();
                if (current_flags & required_flags) != required_flags {
                    pte.set_flags(current_flags | required_flags);
                }
            }
        }
        let pte = unsafe { self.mapping.pte_mut(self.virt, PAGE_TABLE_LEAF_LEVEL, current_frame) };
        let _old_frame = FrameGuard::load(pte).ok();
        frame.store(pte, flags);
        let mut current_frame = self.mapping.page_table_root();
        for level in (PAGE_TABLE_LEAF_LEVEL..=PAGE_TABLE_ROOT_LEVEL).rev() {
            let pte = unsafe { self.mapping.pte_mut(self.virt, level, current_frame) };
            let pte_ptr = NonNull::from(&mut *pte);
            self.nodes[size::from(level)] = Some(pte_ptr);
            if level > PAGE_TABLE_LEAF_LEVEL {
                let is_present = pte.is_present();
                let is_huge = pte.is_huge();
                if is_present && !is_huge {
                    match pte.frame() {
                        Ok(frame) => current_frame = frame,
                        Err(_) => break,
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let page = Page::containing(self.virt);
        unsafe {
            super::mmu::flush(page);
        }

        self.validate();

        Ok(())
    }

    /// Удаляет отображение страницы по текущему пути.
    /// Физический фрейм освобождается, если на него не осталось других ссылок.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- промежуточного или нужного листьевого узла таблицы страниц нет.
    /// - [`Error::Unimplemented`] --- промежуточный узел таблицы страниц
    ///   имеет флаг [`PageTableFlags::HUGE`].
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в удаляемую страницу.
    pub unsafe fn unmap(&mut self) -> Result<()> {
        let leaf_pte = self.get_mut()?;
        let _frame_guard = FrameGuard::load(leaf_pte)?;
        let page = Page::containing(self.virt);
        unsafe {
            super::mmu::flush(page);
        }

        self.validate();

        Ok(())
    }

    /// Возвращает самую далёкую от корня дерева [`PageTableEntry`]
    /// в данном [`Path`] вместе с её номером уровня в дереве отображения.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    /// - На корневом уровне [`PAGE_TABLE_ROOT_LEVEL`] нет [`PageTableEntry`].
    #[allow(clippy::needless_arbitrary_self_type)]
    #[duplicate_item(
        deepest_pte_getter self_type return_type converter;
        [deepest_pte] [&Self] [&'a PageTableEntry] [as_ref];
        [deepest_pte_mut] [&mut Self] [&'a mut PageTableEntry] [as_mut];
    )]
    pub(super) fn deepest_pte_getter(self: self_type) -> (u32, return_type) {
        let level = self.nodes.iter().position(Option::is_some).expect(Self::INVALID_PATH);

        let pte = unsafe { self.nodes[level].expect(Self::INVALID_PATH).converter() };
        let level = level.try_into().expect("unreasonable PTE level");

        (level, pte)
    }

    /// Возвращает блок страниц, который отображает текущий [`Path`].
    /// Аргумент `level` задаёт уровень самой глубокой PTE,
    /// его можно получить из `Path::deepest_pte()`.
    pub(super) fn pages(
        &self,
        level: u32,
    ) -> Block<Page> {
        let page_count = PAGE_TABLE_ENTRY_COUNT.pow(level);

        let end = (Page::containing(self.virt).index() + 1).next_multiple_of(page_count);
        let start = end - page_count;

        Block::from_index(start, end).expect(Self::INVALID_PATH)
    }

    /// Возвращает `true` если путь идёт через рекурсивную запись корневого уровня.
    pub(super) fn is_recursive_entry(&self) -> bool {
        let root_frame = self.mapping.page_table_root();
        let root_pte = unsafe {
            self.nodes[size::from(PAGE_TABLE_ROOT_LEVEL)]
                .expect(Path::INVALID_PATH)
                .as_ref()
        };

        root_pte.frame() == Ok(root_frame)
    }

    /// Проверяет инварианты [`Path`].
    ///
    /// # Panics
    ///
    /// Паникует, если:
    /// - На корневом уровне [`PAGE_TABLE_ROOT_LEVEL`] нет [`PageTableEntry`].
    /// - Выше [`Path::deepest_pte()`] есть [`PageTableEntry`] для которой
    ///   [`PageTableEntry::is_present()`] возвращает `false`.
    /// - Глубже [`Path::deepest_pte()`] есть существующие [`PageTableEntry`].
    /// - Если самая глубокая [`PageTableEntry`] расположена выше [`PAGE_TABLE_LEAF_LEVEL`]
    ///   и при этом для неё [`PageTableEntry::is_present()`] и [`PageTableEntry::is_huge()`]
    ///   возвращают разные значения.
    fn validate(&self) {
        let (level, pte) = self.deepest_pte();
        let level = size::from(level);

        assert!(
            self.nodes[level + 1 ..]
                .iter()
                .all(|pte| unsafe { pte.expect(Self::INVALID_PATH).as_ref().is_present() }),
        );

        if level > size::from(PAGE_TABLE_LEAF_LEVEL) {
            assert!(self.nodes[.. level].iter().all(Option::is_none));
            assert_eq!(pte.is_present(), pte.is_huge());
        }
    }

    /// Сообщение паники при некорректно сформированном [`Path`].
    const INVALID_PATH: &'static str = "invalid path";
}

impl<'a> fmt::Display for Path<'a> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let mut frame = self.mapping.page_table_root();
        let mut level = PAGE_TABLE_ROOT_LEVEL;

        for pte in self.nodes.iter().rev().flatten().map(|node| unsafe { node.as_ref() }) {
            write!(formatter, "L{level}: {} / {frame}", Virt::from_ref(pte))?;

            if !pte.is_present() {
                return write!(formatter, " (non-present)");
            }

            let is_present = "failed to get frame for a present page";

            if pte.is_huge() {
                let (phys, huge_frame_size) = match level {
                    1 => {
                        let huge_frame = pte.huge_frame::<L1_SIZE>().expect(is_present);
                        (huge_frame.offset(self.virt), huge_frame.size())
                    },
                    2 => {
                        let huge_frame = pte.huge_frame::<L2_SIZE>().expect(is_present);
                        (huge_frame.offset(self.virt), huge_frame.size())
                    },
                    _ => panic!("unexpected page table level for a huge page - {}", level),
                };

                return write!(
                    formatter,
                    " ({} huge page) => {phys}",
                    Size::bytes(huge_frame_size),
                );
            };

            frame = pte.frame().expect(is_present);

            if level == PAGE_TABLE_LEAF_LEVEL {
                let phys = frame.offset(self.virt);
                write!(formatter, " => {phys}")?;
            } else {
                write!(formatter, " -> ")?;

                level -= 1;
            }
        }

        Ok(())
    }
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use core::ptr::NonNull;

    use ku::error::Result;

    use super::super::mmu::{
        PAGE_TABLE_LEVEL_COUNT,
        PageTableEntry,
    };

    pub use super::Path;

    pub fn deepest_pte<'a>(path: &'a Path<'a>) -> (u32, &'a PageTableEntry) {
        path.deepest_pte()
    }

    pub fn get_pte<'a>(path: &'a Path<'a>) -> Result<&'a PageTableEntry> {
        path.get()
    }

    pub fn nodes(path: &Path) -> [Option<NonNull<PageTableEntry>>; PAGE_TABLE_LEVEL_COUNT] {
        path.nodes
    }
}
