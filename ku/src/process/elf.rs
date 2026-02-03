use core::{
    cmp::{
        self,
        Ordering,
    },
    mem::MaybeUninit,
    ops::Range,
};

use derive_more::Display;
use xmas_elf::{
    ElfFile,
    program::{
        FLAG_R,
        FLAG_W,
        FLAG_X,
        Flags,
        ProgramHeader,
        Type,
    },
};

use crate::{
    allocator::{
        BigAllocator,
        BigAllocatorPair,
    },
    error::{
        Error::{
            Elf,
            InvalidArgument,
            Overflow,
            PermissionDenied,
        },
        Result,
    },
    log::{
        debug,
        trace,
        warn,
    },
    memory::{
        Block,
        Page,
        Virt,
        block::Memory,
        mmu::PageTableFlags,
        size,
    },
};

// Used in docs.
#[allow(unused)]
use crate::error::Error;

// ANCHOR: load
/// Загружает [ELF--файл](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) `file`.
/// Выделяет для него память в целевом адресном пространстве с помощью `allocator.dst()`.
///
/// Использует разновидность алгоритма
/// [сканирующей прямой](https://ru.algorithmica.org/cs/decomposition/scanline/),
/// обрабатывая сегменты --- [`ProgramHeader`] --- загружаемого файла.
/// При этом опирается на то, что в корректном ELF--файле
/// они должны быть отсортированы по своим виртуальным адресам.
/// См.
/// [System V Application Binary Interface](https://refspecs.linuxbase.org/elf/gabi4+/ch5.pheader.html).
///
/// # Safety
///
/// Вызывающая функция должна гарантировать,
/// что `allocator.src()` выделяет память в текущем адресном пространстве.
pub unsafe fn load<T: BigAllocatorPair>(
    allocator: &mut T,
    file: &[u8],
) -> Result<Virt> {
    // ANCHOR_END: load
    let elf_file = ElfFile::new(file).map_err(|e| Elf(e))?;
    
    let mut loader = Loader::new(allocator, file);
    
    for program_header in elf_file.program_iter() {
        if program_header.get_type().map_err(|e| Elf(e))? == Type::Load {
            let file_range = FileRange::try_from(program_header)?;
            
            debug!(
                next = %file_range,
                "ELF program header",
            );
            
            loader.load_program_header(file_range)?;
        }
    }
    
    loader.finish()?;
    
    let entry_point = Virt::new_u64(elf_file.header.pt2.entry_point())?;
    
    Ok(entry_point)
}

// ANCHOR: loader
/// Состояние загрузчика ELF--файлов.
struct Loader<'a, T: BigAllocatorPair> {
    /// Пара страничных аллокаторов памяти для работы как с целевым адресным пространством,
    /// куда загружается ELF--файл, так и с текущим адресным пространством.
    allocator: &'a mut T,

    /// Текущий обрабатываемый диапазон памяти загружаемого процесса.
    curr: Option<VirtRange>,

    /// Содержимое [ELF--файла](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format).
    file: &'a [u8],

    /// Текущий блок отображённых на физическую память страниц в текущем адресном пространстве.
    /// В него копируются данные из ELF--файла,
    /// после чего соответствующие физические фреймы переносятся в целевое адресное пространство.
    src_block: Block<Page>,
    
    src_block_start_addr: Option<Virt>,
}
// ANCHOR_END: loader

impl<'a, T: BigAllocatorPair> Loader<'a, T> {
    /// Инициализирует состояние загрузчика ELF--файлов.
    fn new(
        allocator: &'a mut T,
        file: &'a [u8],
    ) -> Self {
        Self {
            allocator,
            curr: None,
            file,
            src_block: Block::default(),
            src_block_start_addr: None,
        }
    }

    // ANCHOR: load_program_header
    /// Загружает диапазон [`next`][FileRange] со следующим сегментом
    /// [ELF--файла](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format).
    ///
    /// - Выделяет для него память с помощью [`Loader::extend_mapping()`].
    /// - Копирует его в текущее адресное пространство с помощью [`Loader::copy_to_memory()`].
    /// - С помощью функции [`combine()`] разбивает диапазоны памяти [`Loader::curr`] и `next`
    ///   на выровненные по границам страниц части,
    ///   про которые стали известны окончательные значения флагов отображения.
    ///   И диапазон памяти, флаги которого ещё могут измениться.
    /// - С помощью метода [`Loader::finalize_mapping()`] переносит страницы в
    ///   целевое адресное пространство с правильными флагами отображения
    ///   той части [`Loader::curr`] и границы между [`Loader::curr`] и `next`,
    ///   про которые стали известны окончательные значения флагов отображения.
    /// - Сохраняет в [`Loader::curr`] часть диапазона `next`,
    ///   флаги которого ещё могут измениться.
    ///   То есть, уже не обязательно соответствующую одному сегменту ELF--файла.
    fn load_program_header(
        &mut self,
        next: FileRange,
    ) -> Result<()> {
        // ANCHOR_END: load_program_header
        self.extend_mapping(&next)?;
        let next_virt = self.copy_to_memory(next)?;
        
        if let Some(curr) = self.curr {
            let (curr_minus_next, boundary, updated_next) = combine(curr, next_virt)?;
            
            if let Some(page_range) = curr_minus_next {
                self.finalize_mapping(page_range)?;
            }
            
            if let Some(page_range) = boundary {
                self.finalize_mapping(page_range)?;
            }
            
            self.curr = Some(updated_next);
        } else {
            self.curr = Some(next_virt);
        }
        
        Ok(())
    }

    // ANCHOR: finish
    /// Завершает загрузку ELF--файла в адресное пространство процесса.
    /// После вызова [`Loader::finish()`] в адресное пространство процесса
    /// больше не будут добавляться новые сегменты методом [`Loader::load_program_header()`].
    ///
    /// С помощью метода [`Loader::finalize_mapping()`] переносит оставшиеся страницы из
    /// текущего адресного пространства в целевое для оставшегося диапазона [`Loader::curr`].
    fn finish(&mut self) -> Result<()> {
        // ANCHOR_END: finish
        if let Some(curr) = self.curr.take() {
            let page_range: PageRange = curr.into();
            self.finalize_mapping(page_range)?;
        }
        Ok(())
    }

    // ANCHOR: copy_to_memory
    /// Копирует из ELF--файла [`Loader::file`] в текущее адресное пространство
    /// диапазон памяти [`range`][FileRange].
    ///
    /// Диапазон страниц в текущем адресном пространстве, в который осуществляется копирование,
    /// задаётся [`Loader::src_block`].
    fn copy_to_memory(
        &mut self,
        range: FileRange,
    ) -> Result<VirtRange> {
        // ANCHOR_END: copy_to_memory
        let file_data = self.file.get(range.file_range.clone()).ok_or(Overflow)?;
        
        // Calculate the destination address in src_block
        // src_block_start_addr tracks the virtual address corresponding to src_block.start()
        let dst_start = self.src_block_start_addr.expect("src_block_start_addr must be initialized in extend_mapping");
        
        let src_offset = range.virt_range.memory.start_address().into_usize() - dst_start.into_usize();
        let dst_slice = unsafe {
            self.src_block.try_into_mut_slice::<u8>()?
        };
        
        let copy_len = file_data.len();
        dst_slice[src_offset..src_offset + copy_len].copy_from_slice(file_data);
        
        let mem_size = range.virt_range.memory.size();
        if copy_len < mem_size {
            let zero_start = src_offset + copy_len;
            let zero_end = src_offset + mem_size;
            dst_slice[zero_start..zero_end].fill(0);
        }
        
        Ok(range.virt_range)
    }

    // ANCHOR: extend_mapping
    /// Расширяет отображение целевого адресного пространства,
    /// чтобы гарантировать что блок, который описывает `next`, зарезервирован в памяти.
    ///
    /// - Уже зарезервированную часть памяти определяет по [`Loader::curr`],
    ///   для которого вызывалась ранее. Это должен обеспечить вызывающий метод.
    /// - Резервирует в целевом адресном пространстве виртуальные страницы для `next`
    ///   методом [`BigAllocator::reserve_fixed()`].
    /// - Резервирует, отображает в физическую память методом [`BigAllocator::map()`]
    ///   и зануляет с помощью [`slice::write_filled()`] необходимое количество страниц
    ///   в текущем адресном пространстве.
    ///   В них позже будут временно скопированы данные из ELF--файла.
    ///   После чего они будут перенесены в зарезервированную область
    ///   целевого адресного пространства.
    /// - Флаги страниц в текущем адресном пространстве должны быть выставлены так,
    ///   чтобы в эту память мог записать метод
    ///   [`Loader::copy_to_memory()`].
    ///   Кроме нужных для копирования флагов требуется также указывать
    ///   флаги аллокатора [`Loader::allocator.src().flags()`][BigAllocator::flags]
    ///   текущего адресного пространства.
    ///   Среди них, например, может быть [`PageTableFlags::USER`].
    fn extend_mapping(
        &mut self,
        next: &FileRange,
    ) -> Result<()> {
        // ANCHOR_END: extend_mapping
        let next_page_block = next.virt_range.memory.enclosing();
        let dst_new_pages = if let Some(curr) = &self.curr {
            let curr_page_block = curr.memory.enclosing();

            if curr_page_block.is_disjoint(next_page_block) {
                next_page_block
            } else {
                let overlap_end = cmp::min(curr_page_block.end(), next_page_block.end());
                if overlap_end < next_page_block.end() {
                    Block::<Page>::from_index(overlap_end, next_page_block.end())?
                } else {
                    Block::default()
                }
            }
        } else {
            self.src_block_start_addr = Some(next_page_block.start_address());
            next_page_block
        };
        
        if !dst_new_pages.is_empty() {
            self.allocator.dst().reserve_fixed(dst_new_pages)?;
        }
        
        let src_block_start = Page::containing(self.src_block_start_addr.unwrap());
        let total_pages_needed = next_page_block.end() - src_block_start.index();
        
        if total_pages_needed > self.src_block.count() {
            let src_flags = self.allocator.src().flags() | PageTableFlags::WRITABLE;
            let new_src_block = unsafe {
                self.allocator.src().grow(
                    self.src_block,
                    Page::layout_array(total_pages_needed),
                    src_flags,
                )?
            };
            
            let new_pages_slice = unsafe {
                Block::<Page>::from_index(
                    new_src_block.start() + self.src_block.count(),
                    new_src_block.end(),
                )?.try_into_mut_slice::<u8>()?
            };
            new_pages_slice.fill(0);
            
            self.src_block = new_src_block;
        }
        
        Ok(())
    }

    // ANCHOR: finalize_mapping
    /// Переносит заполненные физические фреймы для целевого диапазона
    /// [`page_range`][PageRange] из текущего адресного пространства в целевое
    /// с окончательными флагами.
    ///
    /// Кроме заданных в ELF--файле флагов требуется также указывать
    /// флаги аллокатора [`Loader::allocator.dst().flags()`][BigAllocator::flags].
    /// Среди них, например, может быть [`PageTableFlags::USER`],
    /// которого нет во флагах ELF--файла.
    fn finalize_mapping(
        &mut self,
        page_range: PageRange,
    ) -> Result<()> {
        // ANCHOR_END: finalize_mapping
        if page_range.memory.is_empty() {
            return Ok(());
        }
        
        let src_start = self.src_block.start();
        let dst_start = Page::containing(self.src_block_start_addr.unwrap());
        
        let offset = page_range.memory.start() - dst_start.index();
        let src_block = Block::<Page>::from_index(
            src_start + offset,
            src_start + offset + page_range.memory.count(),
        )?;
        
        let final_flags = page_range.flags | self.allocator.dst().flags();
        
        debug!(
            page_range = %page_range,
            "remap ELF page range",
        );
        
        unsafe {
            self.allocator.copy_mapping(src_block, page_range.memory, Some(final_flags))?;
        }
        
        Ok(())
    }
}

// ANCHOR: memory_range
/// Диапазон памяти процесса с заданными флагами доступа.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[display("{{ memory: {}, flags: {} }}", memory, flags)]
struct MemoryRange<T: Memory<Address = Virt>> {
    /// Флаги, с которыми должен быть отображён этот диапазон памяти процесса.
    flags: PageTableFlags,

    /// Диапазон памяти процесса, который нужно отобразить в адресное пространство процесса.
    memory: Block<T>,
}
// ANCHOR_END: memory_range

/// Не выровненный по границам страниц диапазон памяти,
/// который нужно отобразить в адресное пространство процесса.
type VirtRange = MemoryRange<Virt>;

/// Выровненный по границам страниц диапазон памяти,
/// который нужно отобразить в адресное пространство процесса.
type PageRange = MemoryRange<Page>;

impl From<VirtRange> for PageRange {
    /// Возвращает выровненный по границам страниц диапазон памяти [`PageRange`],
    /// содержащий [`self`][VirtRange] и имеющий те же флаги отображения страниц.
    fn from(virt_range: VirtRange) -> Self {
        Self {
            flags: virt_range.flags,
            memory: virt_range.memory.enclosing(),
        }
    }
}

// ANCHOR: file_range
/// Не выровненный по границам страниц диапазон памяти,
/// который нужно скопировать из ELF--файла в адресное пространство процесса.
///
/// Так как этот диапазон не выровнен по границам страниц,
/// он ещё не готов к отображению в память с финальными флагами,
/// которые диктует ELF--файл.
///
/// Создаётся по [`ProgramHeader`] в методе [`FileRange::try_from()`].
/// То есть, изначально соответствует одному загружаемому сегменту ELF--файла.
/// Но в процессе загрузки ELF--файла может быть как объединён со смежными сегментами,
/// так и разделён на части типа [`PageRange`] и [`VirtRange`].
/// См. функцию [`combine()`].
#[derive(Debug, Display, Eq, PartialEq)]
#[display("{{ {}, file_range: {:#X?} }}", virt_range, file_range)]
struct FileRange {
    /// Диапазон байт файла, которые нужно скопировать в память процесса.
    file_range: Range<usize>,

    /// Не выровненный по границам страниц диапазон памяти,
    /// который нужно отобразить в адресное пространство процесса.
    virt_range: VirtRange,
}
// ANCHOR_END: file_range

impl TryFrom<ProgramHeader<'_>> for FileRange {
    type Error = Error;

    /// Преобразует сегмент [`program_header`][ProgramHeader]
    /// [ELF--файла](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)
    /// в соответствующий ему описатель блока памяти,
    /// которую нужно скопировать в адресное пространство процесса.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если какие-то адреса в сегменте не являются
    ///     [каноническими](https://en.wikipedia.org/wiki/X86-64#Canonical_form_addresses).
    ///   - [`Error::Overflow`] если:
    ///     - Диапазон в памяти не корректен или меньше чем диапазон в файле, см.
    ///       [System V Application Binary Interface](https://refspecs.linuxbase.org/elf/gabi4+/ch5.pheader.html).
    ///     - При вычислении диапазона [`FileRange::file_range`],
    ///       отвечающего содержимому сегмента в ELF--файле,
    ///       возникает переполнение.
    ///   - [`Error::PermissionDenied`] если входные флаги содержат одновременно
    ///     [`FLAG_W`] и [`FLAG_X`] --- то есть открывают возможность атакующему
    ///     [записать и выполнить произвольный код](https://en.wikipedia.org/wiki/Arbitrary_code_execution)
    ///     внутри создаваемого процесса.
    fn try_from(program_header: ProgramHeader) -> Result<Self> {
        let ph_flags = program_header.flags();
        let flags = PageTableFlags::try_from(ph_flags)?;
        
        let virt_addr = program_header.virtual_addr();
        let mem_size = program_header.mem_size();
        let file_size = program_header.file_size();
        let file_offset = program_header.offset();
        
        if file_size > mem_size {
            return Err(Overflow);
        }
        
        let start_virt = Virt::new_u64(virt_addr)?;
        let end_virt = (start_virt + size::from(mem_size))?;
        let memory = Block::<Virt>::new(start_virt, end_virt)?;
        
        let file_start = size::from(file_offset);
        let file_end = file_start.checked_add(size::from(file_size)).ok_or(Overflow)?;
        
        Ok(Self {
            file_range: file_start..file_end,
            virt_range: VirtRange {
                flags,
                memory,
            },
        })
    }
}

// ANCHOR: combine
/// Объединяет не выровненные по границам страниц
/// текущий обрабатываемый диапазон [`curr`][VirtRange] и
/// следующий за ним диапазон [`next`][VirtRange].
///
/// Требует, чтобы `curr` лежал левее `next` в диапазоне адресов.
/// А также, чтобы они не пересекались.
/// См.
/// [System V Application Binary Interface](https://refspecs.linuxbase.org/elf/gabi4+/ch5.pheader.html)
/// и функцию [`validate_order()`].
/// В противном случае, возвращает ошибку [`Error::InvalidArgument`].
///
/// Возвращает `(curr_minus_next, boundary, updated_next)`, где:
///   - `curr_minus_next` --- выровненная по границам страниц максимальная часть `curr`,
///     которая не пересекается с `next` и последующими сегментами ELF--файла.
///     То есть, `curr_minus_next` гарантированно не попадает в те же страницы,
///     в которые попадает `next` или могут попасть последующие сегменты ELF--файла.
///     Про неё уже известны окончательные флаги отображения, и они совпадают с флагами `curr`.
///     Может быть равна [`None`].
///   - `boundary` --- пограничная страница, в которую попадает как `curr`,
///     так и `next`. Но при этом не могут попасть никакие последующие сегменты ELF--файла.
///     Флаги в ней должны быть подходящими как для диапазона `curr`, так и для диапазона `next`.
///     Поэтому они могут отличаться от флагов `curr_minus_next` и поэтому эта пограничная
///     страница не может быть обработана в составе `curr_minus_next`.
///     Часть `boundary` может содержать только одну страницу памяти.
///     Или же она равна [`None`], если никакие части `curr` и `next` не попадают в одну и ту же
///     страницу, либо в эту же страницу потенциально могут попасть последующие сегменты ELF--файла.
///   - `updated_next` содержит части `curr` и `next`,
///     которые не попали в `curr_minus_next` и `boundary`.
///     Флаги в ней должны быть подходящими для диапазона `next`.
///     В случае, если `curr` задевает те же страницы,
///     флаги `updated_next` должны быть подходящими и для `curr`.
///     В этом случае `updated_next` не может задевать больше одной страницы.
fn combine(
    curr: VirtRange,
    next: VirtRange,
) -> Result<(Option<PageRange>, Option<PageRange>, VirtRange)> {
    // ANCHOR_END: combine
    validate_order(&curr, &next)?;
    
    let curr_page_block = curr.memory.enclosing();
    let next_page_block = next.memory.enclosing();
    
    let combined_flags = curr.flags | next.flags;
    
    debug!(
        %curr, %next,
        curr_page_block = %curr_page_block,
        next_page_block = %next_page_block,
        disjoint = %curr_page_block.is_disjoint(next_page_block),
        "combine input"
    );
    
    if curr_page_block.is_disjoint(next_page_block) {
        let curr_minus_next = Some(PageRange {
            flags: curr.flags,
            memory: curr_page_block,
        });
        debug!(
            curr_minus_next = ?curr_minus_next,
            "combine result: disjoint case"
        );
        return Ok((curr_minus_next, None, next));
    }
    
    let overlap_start = cmp::max(curr_page_block.start(), next_page_block.start());
    let overlap_end = cmp::min(curr_page_block.end(), next_page_block.end());
    
    let curr_minus_next = if curr_page_block.start() < overlap_end - 1 {
        Some(PageRange {
            flags: curr.flags,
            memory: Block::from_index(curr_page_block.start(), overlap_end - 1)?,
        })
    } else {
        None
    };

    let boundary = if overlap_start < overlap_end {
        let last_overlap_page = Page::from_index(overlap_end - 1)?;
        let last_overlap_page_end = (last_overlap_page + 1)?.address();
        
        if next.memory.end_address()? > last_overlap_page_end {
            Some(PageRange {
                flags: combined_flags,
                memory: Block::from_index(overlap_end - 1, overlap_end)?,
            })
        } else {
            None
        }
    } else {
        None
    };
    
    let updated_next_start_addr = if boundary.is_some() {
        Page::from_index(overlap_end)?.address()
    } else if curr_minus_next.is_some() {
        Page::from_index(overlap_end - 1)?.address()
    } else {
        curr.memory.start_address()
    };
    
    let next_end_addr = next.memory.end_address()?;
    
    let updated_next_memory = Block::<Virt>::new(updated_next_start_addr, next_end_addr)?;
    
    let updated_next_flags = if boundary.is_none() && overlap_start < overlap_end {
        combined_flags
    } else {
        next.flags
    };
    
    let updated_next = VirtRange {
        flags: updated_next_flags,
        memory: updated_next_memory,
    };
    
    debug!(
        curr_minus_next = ?curr_minus_next,
        boundary = ?boundary,
        %updated_next,
        updated_next_page_count = %updated_next.memory.enclosing().count(),
        "combine result: overlapping case"
    );
    
    debug!(
        "combine details: overlap_start={}, overlap_end={}, updated_next_start_addr={:?}, next.end_addr={:?}",
        overlap_start, overlap_end,
        updated_next_start_addr,
        next.memory.end_address()
    );
    
    Ok((curr_minus_next, boundary, updated_next))
}

impl TryFrom<Flags> for PageTableFlags {
    type Error = Error;

    /// Переводит флаги доступа сегментов ELF--файла в соответствующие [`PageTableFlags`].
    ///
    /// Возвращает ошибку [`Error::PermissionDenied`] если:
    ///   - Входные флаги содержат одновременно [`FLAG_W`] и [`FLAG_X`] ---
    ///     то есть открывают возможность атакующему
    ///     [записать и выполнить произвольный код](https://en.wikipedia.org/wiki/Arbitrary_code_execution)
    ///     внутри создаваемого процесса.
    fn try_from(ph_flags: Flags) -> Result<Self> {
        let Flags(ph_flags) = ph_flags;

        let insecure_flags = FLAG_W | FLAG_X;
        if ph_flags & insecure_flags == insecure_flags {
            return Err(PermissionDenied);
        }

        let mut flags = PageTableFlags::PRESENT;

        for (ph_flag, flag) in [
            (FLAG_R, PageTableFlags::default()),
            (FLAG_W, PageTableFlags::WRITABLE),
            (FLAG_X, PageTableFlags::EXECUTABLE),
        ] {
            if ph_flags & ph_flag != 0 {
                flags ^= flag;
            }
        }

        Ok(flags)
    }
}

/// Требует чтобы [`curr`][VirtRange] лежал левее [`next`][VirtRange] в диапазоне адресов,
/// А также, чтобы они не пересекались.
/// См.
/// [System V Application Binary Interface](https://refspecs.linuxbase.org/elf/gabi4+/ch5.pheader.html).
/// В противном случае, возвращает ошибку [`Error::InvalidArgument`].
fn validate_order(
    curr: &VirtRange,
    next: &VirtRange,
) -> Result<()> {
    if curr.memory.partial_cmp(&next.memory) != Some(Ordering::Less) {
        warn!(
            curr_program_header = %curr.memory,
            next_program_header = %next.memory,
            "ELF loadable program headers intersect or are out of order by their virtual addresses",
        );
        Err(InvalidArgument)
    } else {
        Ok(())
    }
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use core::ops::Range;

    use derive_more::Display;
    use serde::{
        Deserialize,
        Serialize,
    };
    use xmas_elf::program::{
        ProgramHeader,
        ProgramHeader64,
    };

    use crate::{
        allocator::BigAllocatorPair,
        error::Result,
        memory::{
            Block,
            Page,
            Virt,
            block::Memory,
            mmu::PageTableFlags,
        },
    };

    pub struct Loader<'a, T: BigAllocatorPair>(super::Loader<'a, T>);

    impl<'a, T: BigAllocatorPair> Loader<'a, T> {
        pub fn new(
            allocator: &'a mut T,
            file: &'a [u8],
        ) -> Self {
            Self(super::Loader::new(allocator, file))
        }

        pub fn load_program_header(
            &mut self,
            next: FileRange,
        ) -> Result<()> {
            let next = next.into();
            self.0.load_program_header(next)
        }

        pub fn finish(&mut self) -> Result<()> {
            self.0.finish()
        }

        pub fn extend_mapping(
            &mut self,
            next: &FileRange,
        ) -> Result<()> {
            let next = next.clone().into();
            self.0.extend_mapping(&next)
        }

        pub fn finalize_mapping(
            &mut self,
            page_range: PageRange,
        ) -> Result<()> {
            self.0.finalize_mapping(page_range.into())
        }
    }

    #[derive(Clone, Copy, Debug, Default, Deserialize, Display, Eq, PartialEq, Serialize)]
    #[display("{{ memory: {}, flags: {} }}", memory, flags)]
    pub struct MemoryRange<T: Memory<Address = Virt>> {
        pub flags: PageTableFlags,
        pub memory: Block<T>,
    }

    impl<T: Memory<Address = Virt>> MemoryRange<T> {
        pub fn new(
            memory: Block<T>,
            flags: PageTableFlags,
        ) -> Self {
            Self { flags, memory }
        }

        pub fn start_address(&self) -> Virt {
            self.memory.start_address()
        }

        pub fn end_address(&self) -> Result<Virt> {
            self.memory.end_address()
        }
    }

    impl<T: Memory<Address = Virt>> From<MemoryRange<T>> for super::MemoryRange<T> {
        fn from(memory_range: MemoryRange<T>) -> Self {
            Self {
                flags: memory_range.flags,
                memory: memory_range.memory,
            }
        }
    }

    impl<T: Memory<Address = Virt>> From<super::MemoryRange<T>> for MemoryRange<T> {
        fn from(memory_range: super::MemoryRange<T>) -> Self {
            Self {
                flags: memory_range.flags,
                memory: memory_range.memory,
            }
        }
    }

    pub type VirtRange = MemoryRange<Virt>;

    pub type PageRange = MemoryRange<Page>;

    impl From<VirtRange> for PageRange {
        fn from(virt_range: VirtRange) -> Self {
            super::PageRange::from(super::VirtRange::from(virt_range)).into()
        }
    }

    #[derive(Clone, Debug, Default, Display, Eq, PartialEq)]
    #[display("{{ {}, file_range: {:#X?} }}", virt_range, file_range)]
    pub struct FileRange {
        pub file_range: Range<usize>,
        pub virt_range: VirtRange,
    }

    impl FileRange {
        pub fn new(
            memory: Block<Virt>,
            flags: PageTableFlags,
            file_range: Range<usize>,
        ) -> Self {
            Self {
                file_range,
                virt_range: VirtRange::new(memory, flags),
            }
        }
    }

    impl From<FileRange> for super::FileRange {
        fn from(file_range: FileRange) -> Self {
            Self {
                file_range: file_range.file_range,
                virt_range: super::VirtRange {
                    flags: file_range.virt_range.flags,
                    memory: file_range.virt_range.memory,
                },
            }
        }
    }

    impl From<super::FileRange> for FileRange {
        fn from(file_range: super::FileRange) -> Self {
            Self {
                virt_range: VirtRange::new(
                    file_range.virt_range.memory,
                    file_range.virt_range.flags,
                ),
                file_range: file_range.file_range,
            }
        }
    }

    pub fn program_header_to_file_range(program_header: &ProgramHeader64) -> Result<FileRange> {
        Ok(super::FileRange::try_from(ProgramHeader::Ph64(program_header))?.into())
    }

    pub fn combine(
        curr: VirtRange,
        next: VirtRange,
    ) -> Result<(Option<PageRange>, Option<PageRange>, VirtRange)> {
        let (curr_minus_next, boundary, updated_next) = super::combine(curr.into(), next.into())?;

        Ok((
            curr_minus_next.map(|x| x.into()),
            boundary.map(|x| x.into()),
            updated_next.into(),
        ))
    }
}
