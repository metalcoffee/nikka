use core::{
    fmt,
    result,
};

use ku::memory::{
    Block,
    Frame,
    Page,
    mmu::PageTableFlags,
};

/// Описатель отображённого блока адресного пространства.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct MappedBlock {
    /// Маска флагов, которые не были принудительно сброшены в [`MappedBlock::flags`].
    flag_mask: PageTableFlags,

    /// Флаги, с которыми отображён этот блок.
    ///
    /// Могут не соответствовать всем отображённым страницам,
    /// если этот блок был получен через объединение смежных блоков методом
    /// [`MappedBlock::coalesce()`] с маской флагов, отличной от [`PageTableFlags::all()`].
    flags: PageTableFlags,

    /// Блок физических фреймов, которые использованы в этом отображённом блоке.
    ///
    /// Может быть равен [`Block::default()`],
    /// если этот блок был получен через объединение смежных блоков методом
    /// [`MappedBlock::coalesce()`] с игнорированием адресов фреймов.
    frames: Block<Frame>,

    /// Блок виртуальных страниц, которые отображены в адресном пространстве.
    pages: Block<Page>,
}

impl MappedBlock {
    /// Создаёт описатель отображённого блока адресного пространства
    /// из `pages` в `frames` с флагами `flags`.
    ///
    /// # Panics
    ///
    /// Паникует, если `frames` не равен [`Block::default()`] и
    /// не соответствует `pages` по размеру.
    pub(super) fn new(
        flags: PageTableFlags,
        frames: Block<Frame>,
        pages: Block<Page>,
    ) -> Self {
        assert!(frames == Block::default() || frames.count() == pages.count());

        Self {
            flag_mask: PageTableFlags::all(),
            flags,
            frames,
            pages,
        }
    }

    /// Возвращает `true`, если этот блок отображён в физическую память.
    pub fn is_present(&self) -> bool {
        self.flags.is_present()
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Объединяет блоки [`MappedBlock`], если они смежные в виртуальном пространстве
    /// и `other` лежит правее текущего в виртуальных адресах. Кроме того:
    ///   - Если `ignore_frame_addresses == false`,
    ///     в физическом пространстве блоки тоже должны быть смежными,
    ///     а `other` должен лежать правее текущего в физических адресах.
    ///   - У блоков должны совпадать флаги [`MappedBlock::flags`]
    ///     после пересечения с `flag_mask`.
    ///     Например, это позволяет объединять в единые блоки страницы, часть которых помечена
    ///     [`PageTableFlags::ACCESSED`] или [`PageTableFlags::DIRTY`], а часть --- нет.
    ///
    /// См. также [`Block::coalesce()`].
    ///
    /// # Panics
    ///
    /// Паникует, если сравниваемые блоки были ранее объединены с помощью `coalesce()`
    /// с аргументами, допускающими более широкое объединение.
    /// Например, если раньше они были объединены с `prev_ignore_frame_addresses` и
    /// `prev_flag_mask`, и при этом:
    ///   - `prev_ignore_frame_addresses && !ignore_frame_addresses`;
    ///   - или `prev_flag_mask != flag_mask && flag_mask.contains(prev_flag_mask)`.
    pub fn coalesce(
        mut self,
        mut other: Self,
        ignore_frame_addresses: bool,
        flag_mask: PageTableFlags,
    ) -> result::Result<Self, (Self, Self)> {
        if (!ignore_frame_addresses && (self.frames.is_empty() || other.frames.is_empty())) ||
            flag_mask & self.flag_mask & other.flag_mask != flag_mask
        {
            panic!("strictness of consecutive coalesces should not increase");
        }

        let flags = self.flags & flag_mask;

        let frames = if ignore_frame_addresses {
            Ok(Block::default())
        } else if flags.is_present() {
            self.frames.coalesce(other.frames)
        } else {
            Block::from_index(0, self.frames.count() + other.frames.count())
        };

        let pages = self.pages.coalesce(other.pages);

        if let Ok(pages) = pages &&
            let Ok(frames) = frames &&
            flags == other.flags & flag_mask
        {
            Ok(Self {
                flag_mask,
                flags,
                frames,
                pages,
            })
        } else {
            for mapped_block in [&mut self, &mut other] {
                mapped_block.flag_mask = flag_mask;
                mapped_block.flags &= flag_mask;

                if ignore_frame_addresses {
                    mapped_block.frames = Block::default();
                }
            }

            Err((self, other))
        }
    }
}

impl fmt::Display for MappedBlock {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}", self.pages)?;
        if !self.frames.is_empty() {
            write!(formatter, " -> {}", self.frames)?;
        }
        write!(formatter, ", {}", self.flags)
    }
}
