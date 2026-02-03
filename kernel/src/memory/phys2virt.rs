use core::{
    fmt,
    mem::MaybeUninit,
    num::NonZeroUsize,
};

use itertools::Itertools;

use ku::{
    error::{
        Error::{
            InvalidArgument,
            NoPage,
            Overflow,
            PermissionDenied,
        },
        Result,
    },
    memory::{
        Block,
        Frame,
        L2_SIZE,
        L2Frame,
        Page,
        Phys,
        Size,
        Virt,
        mmu::{
            KERNEL_RW,
            PAGE_TABLE_ENTRY_COUNT,
            PAGE_TABLE_ROOT_LEVEL,
            PageTable,
            PageTableEntry,
        },
    },
};

use super::{
    FRAME_ALLOCATOR,
    FrameGuard,
    range,
};

// Used in docs.
#[allow(unused)]
use {
    crate as kernel,
    crate::error::Error,
};

/// Для простоты работы с физической памятью,
/// она целиком линейно отображена в некоторую область виртуальной.
/// [`Phys2Virt`] описывает это отображение.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Phys2Virt {
    /// Блок виртуальной памяти, куда линейно отображена вся физическая память.
    mapping: Block<Page>,

    /// Блок, описывающий всю доступную физическую память
    /// (см. [`kernel::memory::range::physical()`]).
    physical_memory: Block<Frame>,

    /// Количество использованных для отображения физических фреймов.
    /// Или [`None`], если оно не известно, так как отображение построил [`bootloader`].
    used_frame_count: Option<NonZeroUsize>,
}

impl Phys2Virt {
    /// Создаёт [`Phys2Virt`] по его начальной странице `start` и
    /// блоку всей физической памяти `physical_memory`
    /// (см. [`kernel::memory::range::physical()`]).
    pub(super) fn new(
        physical_memory: Block<Frame>,
        start: Page,
    ) -> Result<Self> {
        Self::new_impl(physical_memory, start, None)
    }

    /// Для заданного физического адреса `phys` возвращает виртуальный адрес
    /// внутри специальной области [`Phys2Virt`],
    /// в которую линейно отображена вся физическая память.
    ///
    /// # Errors
    ///
    /// - [`Error::Overflow`] --- адрес `phys` не попадает в физическую память,
    ///   имеющуюся в системе.
    pub fn map(
        &self,
        phys: Phys,
    ) -> Result<Virt> {
        let virt = (self.mapping.start_address() + phys.into_usize())?;
        if self.mapping.contains_address(virt) {
            Ok(virt)
        } else {
            Err(Overflow)
        }
    }

    // ANCHOR: make
    /// Создаёт отображение [`Phys2Virt`] всей физической памяти `physical_memory`
    /// (см. [`kernel::memory::range::physical()`]),
    /// используя рекурсивную запись `recursive_mapping`.
    ///
    /// Для экономии физических фреймов под  [`Phys2Virt`]:
    ///   - Использует страницы размером по 1 GiB.
    ///   - Учитывает, что фреймы вне [`Phys2Virt::physical_memory`]
    ///     никогда не будут переданы в [`Phys2Virt::map()`].
    ///     Так как их нет в физической памяти.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidArgument`] --- номер рекурсивной записи `recursive_mapping`
    ///   выходит за пределы узла [`PageTable`] дерева отображения страниц.
    ///   Или равен нулю, чего не может быть так как под нулевой записью верхнего уровня есть
    ///   зарезервированные участки памяти.
    /// - [`Error::NoFrame`] --- не хватило свободной физической памяти.
    /// - [`Error::NoPage`] --- в корневом узле дерева отображения виртуальных страниц
    ///   не нашлось достаточного количества записей для отображения блока `physical_memory`.
    /// - [`Error::PermissionDenied`] --- рекурсивная запись `recursive_mapping` попадает в
    ///   пользовательскую часть виртуальной памяти.
    pub(super) fn make(
        physical_memory: Block<Frame>,
        recursive_mapping: usize,
    ) -> Result<Self> {
        // ANCHOR_END: make
        if recursive_mapping == 0 || recursive_mapping >= PAGE_TABLE_ENTRY_COUNT {
            return Err(InvalidArgument);
        }
        if range::user_root_level_entries().contains(&recursive_mapping) {
            return Err(PermissionDenied);
        }
        let entry_count = physical_memory.size().div_ceil(1usize << 30);
        let root_entry_count = entry_count.div_ceil(PAGE_TABLE_ENTRY_COUNT);
        let start_root_entry = PAGE_TABLE_ENTRY_COUNT / 2;
        if start_root_entry + root_entry_count > PAGE_TABLE_ENTRY_COUNT {
            return Err(NoPage);
        }
        Self::fill_entries(physical_memory, start_root_entry, root_entry_count, entry_count, recursive_mapping)?;
        let used_frame_count = NonZeroUsize::new(root_entry_count)
            .expect("root_entry_count should be non-zero");
        Self::new_impl(physical_memory, Page::higher_half(), Some(used_frame_count))
    }

    /// Инициализирует записи 1 GiB страниц для отображения [`Phys2Virt`].
    ///
    /// Начинает с корневой записи `start_root_entry` и
    /// инициализирует `root_entry_count` корневых записей.
    /// А также инициализирует `entry_count` записей следующего уровня,
    /// каждая из которых отвечает за 1 GiB виртуальной памяти.
    ///
    /// Использует рекурсивную запись номер `recursive_mapping`.
    ///
    /// # Errors
    ///
    /// - [`Error::NoFrame`] --- не хватило свободной физической памяти.
    fn fill_entries(
        physical_memory: Block<Frame>,
        start_root_entry: usize,
        root_entry_count: usize,
        entry_count: usize,
        recursive_mapping: usize,
    ) -> Result<()> {
        for i in 0..root_entry_count {
            let root_entry_index = start_root_entry + i;
            Self::allocate_root_entry(root_entry_index, recursive_mapping)?;
        }
        let mut entries_filled = 0;
        for i in 0..root_entry_count {
            let root_entry_index = start_root_entry + i;
            let node = unsafe {
                Self::node(root_entry_index, recursive_mapping)
                    .try_into_mut_slice::<PageTableEntry>()?
            };
            let entries_to_fill = core::cmp::min(
                entry_count - entries_filled,
                PAGE_TABLE_ENTRY_COUNT
            );
            
            for j in 0..entries_to_fill {
                let frame_index = entries_filled + j;
                if frame_index < entry_count {
                    let phys_addr = (physical_memory.start_address() + (frame_index * (1usize << 30)))?;
                    let l2_frame = L2Frame::new(phys_addr).expect("Failed to create L2 frame");
                    node[j].set_huge_frame::<L2_SIZE>(l2_frame, KERNEL_RW);
                }
            }
            for j in entries_to_fill..PAGE_TABLE_ENTRY_COUNT {
                node[j] = PageTableEntry::default();
            }
            
            entries_filled += entries_to_fill;
            if entries_filled >= entry_count {
                break;
            }
        }
        
        Ok(())
    }

    /// Выделяет фрейм для дочернего узла номер `root_entry` корневого узла таблицы страниц,
    /// инициализирует его пустыми записями ---- [`PageTableEntry::default()`] --- и
    /// записывает его в соответствующую [`PageTableEntry`].
    ///
    /// Использует рекурсивную запись номер `recursive_mapping`.
    ///
    /// # Errors
    ///
    /// - [`Error::NoFrame`] --- не хватило свободной физической памяти.
    fn allocate_root_entry(
        root_entry: usize,
        recursive_mapping: usize,
    ) -> Result<()> {
        let root = unsafe {
            Self::node(recursive_mapping, recursive_mapping).try_into_mut::<PageTable>()?
        };
        let pte = &mut root[root_entry];

        if !pte.is_present() {
            let frame = FrameGuard::allocate()?;
            frame.store(pte, KERNEL_RW);

            let node = unsafe {
                Self::node(root_entry, recursive_mapping)
                    .try_into_mut_slice::<MaybeUninit<PageTableEntry>>()?
            };
            node.write_filled(PageTableEntry::default());
        } else {
            let node = unsafe {
                Self::node(root_entry, recursive_mapping)
                    .try_into_mut_slice::<PageTableEntry>()?
            };
            node.fill(PageTableEntry::default());
        }

        Ok(())
    }

    /// Возвращает узел таблицы страниц,
    /// на который ссылается запись номер `index` корневого узла таблицы страниц.
    ///   - При `index == recursive_mapping` этот узел --- сам корневой.
    ///   - При `index != recursive_mapping` это узел следующего уровня,
    ///     каждая запись в нём отвечает за 1 GiB адресного пространства.
    ///
    /// Возвращает блок размером в одну 4 KiB страницу,
    /// который удобно преобразовывать с помощью `Block::try_into*()`
    /// в подходящий тип для работы с узлом дерева отображения.
    ///
    /// Использует рекурсивную запись номер `recursive_mapping`.
    fn node(
        index: usize,
        recursive_mapping: usize,
    ) -> Block<Page> {
        let virt = Virt::from_page_table_indexes(
            [
                index,
                recursive_mapping,
                recursive_mapping,
                recursive_mapping,
            ],
            0,
        );
        let message = "wrong calculations through recursive entry";

        let page = Page::new(virt).expect(message);

        Block::new(page, (page + 1).expect(message)).expect(message)
    }

    /// Создаёт [`Phys2Virt`] по его начальной странице `start` и
    /// блоку всей физической памяти `physical_memory`
    /// (см. [`kernel::memory::range::physical()`]).
    ///
    /// Сохраняет `used_frame_count` --- количество физических фреймов,
    /// которые были потрачены на создание отображения `Phys2Virt`.
    /// Или [`None`], если это количество не известно --- отображение создано [`bootloader`].
    fn new_impl(
        physical_memory: Block<Frame>,
        start: Page,
        used_frame_count: Option<NonZeroUsize>,
    ) -> Result<Self> {
        Ok(Self {
            mapping: Block::from_index(start.index(), start.index() + physical_memory.count())?,
            physical_memory,
            used_frame_count,
        })
    }
}

impl fmt::Display for Phys2Virt {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{} -> {}", self.mapping, self.physical_memory)?;

        if let Some(used_frame_count) = self.used_frame_count {
            write!(
                formatter,
                "; uses {} for the mapping itself",
                Size::new::<Frame>(used_frame_count.get()),
            )?;
        }

        Ok(())
    }
}

/// Для простоты работы с ней, физическая память целиком отображена в некоторую область виртуальной.
/// Эта функция по адресу первой (виртуальной) страницы этой области `phys2virt`
/// выдаёт соответствующий виртуальный адрес для заданного физического `address`.
#[allow(unused)]
pub(crate) fn map(
    phys2virt: Page,
    address: Phys,
) -> Virt {
    let message = "bad phys2virt";
    let frame = Frame::containing(address);
    let pseudo_physical_memory =
        Block::new(Frame::default(), (frame + 1).expect(message)).expect(message);

    Phys2Virt::new(pseudo_physical_memory, phys2virt)
        .expect(message)
        .map(address)
        .expect(message)
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use ku::error::Result;

    use super::{
        super::{
            Block,
            Frame,
            Page,
            Phys,
            Virt,
        },
        Phys2Virt,
    };

    pub fn make_phys2virt(
        physical_memory: Block<Frame>,
        recursive_mapping: usize,
    ) -> Result<Phys2Virt> {
        Phys2Virt::make(physical_memory, recursive_mapping)
    }

    pub fn phys2virt_map(
        phys2virt: Page,
        address: Phys,
    ) -> Virt {
        super::map(phys2virt, address)
    }
}
