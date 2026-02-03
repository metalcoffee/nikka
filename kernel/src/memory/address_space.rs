use core::{
    alloc::Layout,
    any,
    fmt,
    mem::MaybeUninit,
    ptr,
};

use duplicate::duplicate_item;
use itertools::Itertools;

use ku::sync::spinlock::Spinlock;

use crate::{
    Subsystems,
    allocator::Big,
    error::{
        Error::{
            InvalidArgument,
            NoPage,
            PermissionDenied,
        },
        Result,
    },
    log::{
        debug,
        info,
        trace,
    },
    process::Pid,
};

use super::{
    FRAME_ALLOCATOR,
    FrameGuard,
    KERNEL_PAGE_ALLOCATOR,
    Path,
    Phys,
    Phys2Virt,
    Translate,
    Virt,
    block::Block,
    frage::{
        Frame,
        Page,
    },
    mapping::Mapping,
    mmu::{
        self,
        PageTableFlags,
    },
    page_allocator::PageAllocator,
    range,
};

// Used in docs.
#[allow(unused)]
use crate::{
    self as kernel,
    error::Error,
};

/// Структура для работы с виртуальным адресным пространством.
#[derive(Debug, Default)]
pub struct AddressSpace {
    /// Тип виртуального адресного пространства для записи в журнал:
    ///   - [`Kind::Invalid`] --- некорректное;
    ///   - [`Kind::Base`] --- базовое;
    ///   - [`Kind::Process`] --- относящееся к некоторому процессу.
    kind: Kind,

    /// Структура для управления отображением виртуальных страниц в физические фреймы.
    mapping: Option<Mapping>,

    /// Аллокатор виртуальных страниц.
    user_page_allocator: PageAllocator,

    /// Процесс, к которому относится данное адресное пространство.
    /// Содержит [`None`], если:
    ///   - у процесса ещё нет [`Pid`] или
    ///   - это адресное пространство не относится ни к какому процессу.
    pid: Option<Pid>,
}

impl AddressSpace {
    /// Инициализирует виртуальное адресное пространство страничным отображением,
    /// корневой узел которого задаёт `page_table_root`.
    /// Аргумент [`phys2virt`][Phys2Virt] описывает линейное отображение
    /// физической памяти в виртуальную внутри этого страничного отображения.
    pub(super) fn new(
        page_table_root: Frame,
        phys2virt: Phys2Virt,
        subsystems: Subsystems,
    ) -> Self {
        let mut mapping = Mapping::new(page_table_root, phys2virt);
        if subsystems.contains(Subsystems::VIRT_MEMORY) {
            mapping.remove_recursive_mappings();
        }

        let user_page_allocator = if subsystems.contains(Subsystems::VIRT_MEMORY) {
            PageAllocator::new(
                unsafe { mapping.page_table_ref(mapping.page_table_root()) },
                range::user_root_level_entries(),
            )
        } else {
            PageAllocator::zero()
        };

        let address_space = Self {
            kind: Kind::Base,
            mapping: Some(mapping),
            user_page_allocator,
            pid: None,
        };

        info!(%address_space, "init");

        address_space
    }

    /// Возвращает пустое [`AddressSpace`].
    /// В отличие от [`AddressSpace::default()`] доступна в константном контексте.
    pub(crate) const fn zero() -> Self {
        Self {
            kind: Kind::Base,
            mapping: None,
            user_page_allocator: PageAllocator::zero(),
            pid: None,
        }
    }

    /// Создаёт копию адресного пространства с копией страничного отображения.
    /// См. [`Mapping::duplicate()`].
    pub(crate) fn duplicate(&self) -> Result<Self> {
        let mapping = self.mapping.as_ref().ok_or(InvalidArgument)?.duplicate()?;

        let address_space = Self {
            kind: Kind::Process,
            mapping: Some(mapping),
            user_page_allocator: self.user_page_allocator.duplicate(),
            pid: None,
        };

        info!(%address_space, "duplicate");

        Ok(address_space)
    }

    /// Устанавливает [`AddressSpace::user_page_allocator`] в состояние эквивалентное `original`.
    /// Текущий [`AddressSpace`] должен был быть получен
    /// из `original` методом [`AddressSpace::duplicate()`].
    /// И из него не должно было быть выделено больше страниц, чем из `original`.
    /// Если это не так, возвращается ошибка [`Error::InvalidArgument`].
    pub(crate) fn duplicate_allocator_state(
        &mut self,
        original: &AddressSpace,
    ) -> Result<()> {
        self.user_page_allocator
            .duplicate_allocator_state(&original.user_page_allocator)
    }

    /// Постраничный аллокатор памяти в этом адресном пространстве.
    /// Выделяемая им память будет отображена с флагами `flags`.
    pub fn allocator(
        &mut self,
        flags: PageTableFlags,
    ) -> Big<'_> {
        Big::new(self, flags)
    }

    /// Возвращает страничное отображение данного виртуального адресного пространства.
    pub(super) fn mapping(&mut self) -> Result<&mut Mapping> {
        self.mapping.as_mut().ok_or(InvalidArgument)
    }

    /// Возвращает физический фрейм, в котором хранится корневой узел
    /// страничного отображения данного виртуального адресного пространства.
    pub fn page_table_root(&self) -> Frame {
        self.mapping.as_ref().expect("invalid AddressSpace").page_table_root()
    }

    /// Сохраняет информацию об идентификаторе процесса,
    /// который владеет этим адресным пространством.
    pub(crate) fn set_pid(
        &mut self,
        pid: Pid,
    ) {
        self.pid = Some(pid)
    }

    /// Переключает процессор в это виртуальное адресное пространство.
    pub(crate) fn switch_to(&self) {
        info!(address_space = %self, "switch to");

        unsafe {
            assert_ne!(self.page_table_root(), Frame::default());
            mmu::set_page_table_root(self.page_table_root());
        }
    }

    /// Выделяет блок подряд идущих виртуальных страниц для хранения объекта,
    /// требования к размещению в памяти которого описывает `layout`.
    /// Ни выделения физической памяти, ни создания отображения станиц, не происходит.
    ///
    /// Если выделить заданный размер виртуальной памяти не удалось,
    /// возвращает ошибку [`Error::NoPage`].
    pub fn allocate(
        &mut self,
        layout: Layout,
        flags: PageTableFlags,
    ) -> Result<Block<Page>> {
        if flags.is_user() {
            self.user_page_allocator.allocate(layout)
        } else {
            KERNEL_PAGE_ALLOCATOR.lock().allocate(layout)
        }
    }

    /// Обратный метод к [`AddressSpace::allocate()`].
    /// Освобождает блок виртуальных страниц `block`.
    /// Ни освобождения физической памяти, ни модификации отображения станиц, не происходит.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidArgument`] --- заданный блок не является целиком выделенным.
    /// - [`Error::PermissionDenied`] --- в блоке есть как страницы,
    ///   которые лежат в зарезервированной для пользователя области,
    ///   так и страницы, которые лежат в зарезервированной для ядра области.
    pub fn deallocate(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        if super::is_kernel_block(block) {
            KERNEL_PAGE_ALLOCATOR.lock().deallocate(block)
        } else if super::is_user_block(block) {
            self.user_page_allocator.deallocate(block)
        } else {
            Err(InvalidArgument)
        }
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Отмечает заданный блок виртуальных страниц как используемый.
    /// Ни выделения физической памяти, ни создания отображения станиц, не происходит.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- заданный блок не является целиком свободным.
    /// - [`Error::PermissionDenied`] --- принадлежность пользователю
    ///   какой-нибудь страницы блока не соответствует запрошенным флагам.
    ///   То есть, адрес этой страницы лежит в зарезервированной для пользователя области
    ///   [`kernel::memory::range::user_pages()`], а во `flags` нет доступа пользователю ---
    ///   флага [`PageTableFlags::USER`].
    ///   Либо наоборот, флаг [`PageTableFlags::USER`] есть, но страница не
    ///   лежит в области [`kernel::memory::range::user_pages()`].
    pub fn reserve(
        &mut self,
        pages: Block<Page>,
        flags: PageTableFlags,
    ) -> Result<()> {
        range::validate_block_flags(pages, flags)?;

        if super::is_kernel_block(pages) {
            KERNEL_PAGE_ALLOCATOR.lock().reserve(pages)
        } else if super::is_user_block(pages) {
            self.user_page_allocator.reserve(pages)
        } else {
            Err(PermissionDenied)
        }
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Отображает заданную виртуальную страницу `page` на заданный физический фрейм `frame`
    /// с указанными флагами доступа `flags`.
    ///
    /// Если `page` уже была отображена, то старое отображение удаляется,
    /// а старый физический фрейм освобождается, если на него не осталось других ссылок.
    ///
    /// # Errors
    ///
    /// - [`Error::PermissionDenied`] --- принадлежность пользователю
    ///   страницы `page` не соответствует запрошенным флагам.
    ///   То есть адрес этой страницы лежит в зарезервированной для пользователя области
    ///   [`kernel::memory::range::user_pages()`], а во `flags` нет доступа пользователю ---
    ///   флага [`PageTableFlags::USER`].
    ///   Либо наоборот, флаг [`PageTableFlags::USER`] есть, но страница не
    ///   лежит в области [`kernel::memory::range::user_pages()`].
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в страницу `page`.
    pub unsafe fn map_page_to_frame(
        &mut self,
        page: Page,
        frame: Frame,
        flags: PageTableFlags,
    ) -> Result<()> {
        range::validate_page_flags(page, flags)?;
        let frame_guard = FrameGuard::reference(frame);
        unsafe {
            self.mapping()?.path(page.address()).map(frame_guard, flags)
        }
    }

    /// Выделяет физический фрейм и отображает на него заданную виртуальную страницу `page`
    /// с указанными флагами доступа `flags`.
    /// Подробнее см. [`AddressSpace::map_page_to_frame()`].
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в страницу `page`.
    pub unsafe fn map_page(
        &mut self,
        page: Page,
        flags: PageTableFlags,
    ) -> Result<Frame> {
        let frame = FRAME_ALLOCATOR.lock().allocate()?;
        let frame_value = *frame;
        unsafe {
            self.map_page_to_frame(page, frame_value, flags)?;
        }
        Ok(frame_value)
    }

    /// Удаляет отображение заданной виртуальной страницы `page`.
    /// Физический фрейм освобождается, если на него не осталось других ссылок.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- заданная виртуальная страница не отображена в память.
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в удаляемую страницу.
    pub unsafe fn unmap_page(
        &mut self,
        page: Page,
    ) -> Result<()> {
        unsafe {
            self.mapping()?.path(page.address()).unmap()
        }
    }

    /// Выделяет нужное количество физических фреймов
    /// и отображает в них заданный блок виртуальных страниц `pages`
    /// с заданными флагами доступа `flags`.
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в `pages`.
    pub unsafe fn map_block(
        &mut self,
        pages: Block<Page>,
        flags: PageTableFlags,
    ) -> Result<()> {
        range::validate_block_flags(pages, flags)?;

        for page in pages {
            unsafe {
                self.map_page(page, flags)?;
            }
        }

        Ok(())
    }

    /// Меняет флаги доступа к заданному блоку виртуальных страниц `pages` на `flags`.
    ///
    /// # Errors
    ///
    /// - [`Error::PermissionDenied`] --- принадлежность пользователю
    ///   какой-нибудь страницы блока `pages` не соответствует запрошенным флагам.
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в `pages`.
    pub unsafe fn remap_block(
        &mut self,
        pages: Block<Page>,
        flags: PageTableFlags,
    ) -> Result<()> {
        range::validate_block_flags(pages, flags)?;

        for page in pages {
            self.mapping()?
                .path(page.address())
                .get_mut()?
                .set_flags(flags | PageTableFlags::PRESENT);
        }

        Ok(())
    }

    /// Удаляет отображение заданного блока виртуальных страниц `pages`.
    /// Физические фреймы, на которые не осталось других ссылок, освобождаются.
    ///
    /// # Safety
    ///
    /// Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    /// не будут нарушены.
    /// В частности, не осталось ссылок, которые ведут в `pages`.
    pub unsafe fn unmap_block(
        &mut self,
        pages: Block<Page>,
    ) -> Result<()> {
        range::validate_block(pages)?;

        for page in pages {
            unsafe {
                self.unmap_page(page)?;
            }
        }

        Ok(())
    }

    /// Выделяет нужное количество физических фреймов
    /// и отображает в них срез элементов типа `T` заданного размера `len`
    /// с заданными флагами доступа `flags`.
    ///
    /// Возвращает срез в виде неинициализированной памяти.
    fn map_slice_uninit<T>(
        &mut self,
        len: usize,
        flags: PageTableFlags,
    ) -> Result<&'static mut [MaybeUninit<T>]> {
        let block = self.allocate(Layout::array::<T>(len)?, flags)?;

        unsafe {
            self.map_block(block, flags)?;
        }

        let slice = unsafe { block.try_into_mut_slice()? };

        trace!(
            %block,
            page_count = block.count(),
            slice = format_args!("[{}; {}]", any::type_name::<T>(), slice.len()),
            "mapped a slice",
        );

        Ok(slice)
    }

    /// Выделяет нужное количество физических фреймов
    /// и отображает в них срез элементов типа `T` заданного размера `len`
    /// с заданными флагами доступа `flags`.
    ///
    /// Инициализирует все элементы среза функцией `default`.
    pub fn map_slice<T, F: Fn() -> T>(
        &mut self,
        len: usize,
        flags: PageTableFlags,
        default: F,
    ) -> Result<&'static mut [T]> {
        assert!(flags.is_writable());
        let slice = self.map_slice_uninit(len, flags)?;
        Ok(slice.write_with(|_| default()))
    }

    /// Выделяет нужное количество физических фреймов
    /// и отображает в них срез элементов типа `T` заданного размера `len`
    /// с заданными флагами доступа `flags`.
    ///
    /// Инициализирует память нулями.
    ///
    /// # Safety
    ///
    /// Нулевой битовый шаблон должен быть валидным значением типа `T`,
    /// см. [`MaybeUninit::zeroed()`].
    pub unsafe fn map_slice_zeroed<T>(
        &mut self,
        len: usize,
        flags: PageTableFlags,
    ) -> Result<&'static mut [T]> {
        assert!(flags.is_writable());

        let slice = self.map_slice_uninit(len, flags)?;

        for element in slice.iter_mut() {
            *element = MaybeUninit::zeroed();
        }

        Ok(unsafe { slice.assume_init_mut() })
    }

    /// Удаляет отображение заданного среза `slice`.
    /// Физические фреймы, на которые не осталось других ссылок, освобождаются.
    ///
    /// # Safety
    ///
    /// - Срез должен был быть ранее выделен одним из методов `AddressSpace::map_slice*()`.
    /// - Также вызывающий код должен гарантировать,
    ///   что инварианты управления памятью в Rust'е не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в `slice`.
    pub unsafe fn unmap_slice<T>(
        &mut self,
        slice: &mut [T],
    ) -> Result<()> {
        let ptr_range = slice.as_ptr_range();
        let start = Page::new(Virt::from_ptr(ptr_range.start))?;
        let end = Page::new(Virt::from_ptr(ptr_range.end))?;

        let block = Block::new(start, end)?;
        let page_count = block.count();

        unsafe {
            for element in slice.iter_mut() {
                ptr::drop_in_place(element as *mut T);
            }

            self.unmap_block(block)?;
        }

        trace!(
            addr = %start.address(),
            page_count,
            slice = format_args!("[{}; {}]", any::type_name::<T>(), slice.len()),
            "unmapped a slice",
        );

        Ok(())
    }

    /// Выделяет нужное количество физических фреймов
    /// и отображает в них один элемент типа `T`
    /// с заданными флагами доступа `flags`.
    ///
    /// Инициализирует элемент функцией `default`.
    pub fn map_one<T, F: FnOnce() -> T>(
        &mut self,
        flags: PageTableFlags,
        default: F,
    ) -> Result<&'static mut T> {
        assert!(flags.is_writable());
        let slice = self.map_slice_uninit(1, flags)?;
        slice[0].write(default());
        Ok(unsafe { MaybeUninit::assume_init_mut(&mut slice[0]) })
    }

    /// Удаляет отображение заданного элемента `element`.
    /// Физические фреймы, на которые не осталось других ссылок, освобождаются.
    ///
    /// # Safety
    ///
    /// - Элемент должен был быть ранее выделен методом [`AddressSpace::map_one()`].
    ///   Для дополнительной проверки этого, требует время жизни `'static`.
    /// - Также вызывающий код должен гарантировать, что инварианты управления памятью
    ///   в Rust'е не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в `element`.
    ///   Для дополнительной проверки этого, требует изменяемость ссылки.
    pub unsafe fn unmap_one<T>(
        &mut self,
        element: &'static mut T,
    ) -> Result<()> {
        unsafe { self.unmap_slice(Block::<Virt>::from_mut(element).try_into_mut_slice::<T>()?) }
    }

    /// Проверяет доступность блока виртуальной памяти `block` на чтение и
    /// соответствие заданным флагам доступа `flags`.
    /// Возвращает неизменяемый срез типа `T`, расположенный в блоке `block`.
    ///
    /// Это позволяет объединить в одно действие проверку доступа и упростить последующий доступ
    /// к памяти, которую процесс пользователя указал ядру в системном вызове.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- какая-нибудь страница блока `block` не отображена.
    /// - [`Error::PermissionDenied`] --- какая-нибудь страница отображена,
    ///   но не со всеми запрошенными флагами.
    pub(crate) fn check_permission<T>(
        &mut self,
        block: Block<Virt>,
        flags: PageTableFlags,
    ) -> Result<&'static [T]> {
        self.check_permission_common(&block, flags)?;

        Ok(unsafe { block.try_into_slice()? })
    }

    /// Проверяет доступность блока виртуальной памяти `block` на запись и
    /// соответствие заданным флагам доступа `flags`.
    /// Возвращает изменяемый срез типа `T`, расположенный в блоке `block`.
    ///
    /// Это позволяет объединить в одно действие проверку доступа и упростить последующий доступ
    /// к памяти, которую процесс пользователя указал ядру в системном вызове.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- какая-нибудь страница блока `block` не отображена.
    /// - [`Error::PermissionDenied`] --- какая-нибудь страница отображена,
    ///   но не со всеми запрошенными флагами.
    ///
    /// # Panics
    ///
    /// Паникует, если запрошенные флаги `flags` не содержат разрешения записи.
    pub(crate) fn check_permission_mut<T>(
        &mut self,
        block: Block<Virt>,
        flags: PageTableFlags,
    ) -> Result<&'static mut [T]> {
        assert!(flags.is_writable());

        self.check_permission_common(&block, flags)?;

        Ok(unsafe { block.try_into_mut_slice()? })
    }

    /// Вспомогательный метод для
    /// [`AddressSpace::check_permission()`] и [`AddressSpace::check_permission_mut()`].
    /// Проверяет блок виртуальной памяти `block` на соответствие заданным флагам доступа `flags`.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- какая-нибудь страница блока `block` не отображена.
    /// - [`Error::PermissionDenied`] --- какая-нибудь страница отображена,
    ///   но не со всеми запрошенными флагами.
    fn check_permission_common(
        &mut self,
        block: &Block<Virt>,
        flags: PageTableFlags,
    ) -> Result<()> {
        if !super::is_user_block(block.enclosing()) {
            return Err(PermissionDenied);
        }

        let required_flags = PageTableFlags::PRESENT | PageTableFlags::USER | flags;
        for page in block.enclosing() {
            let pte = self.mapping()?.translate(page.address())?;
            if !pte.flags().contains(required_flags) {
                return Err(PermissionDenied);
            }
        }

        Ok(())
    }

    /// Выводит в журнал карту виртуального адресного пространства.
    pub(crate) fn dump(&mut self) {
        if let Ok(mapping) = self.mapping() {
            debug!("address space:");

            let flag_mask = !(PageTableFlags::ACCESSED | PageTableFlags::DIRTY);
            let ignore_frame_addresses = true;

            for block in mapping
                .iter_mut()
                .map(|path| path.block())
                .filter(|block| block.is_present())
                .coalesce(|a, b| a.coalesce(b, ignore_frame_addresses, flag_mask))
            {
                debug!("    {}", block);
            }
        } else {
            debug!("address space is empty");
        }
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        if let Some(mapping) = &self.mapping &&
            Mapping::current_page_table_root() == mapping.page_table_root()
        {
            let base_address_space = BASE_ADDRESS_SPACE.lock();
            assert_ne!(self.page_table_root(), base_address_space.page_table_root());
            base_address_space.switch_to();
            info!(
                address_space = %self,
                switch_to = %base_address_space,
                "drop the current address space",
            );
        } else {
            info!(address_space = %self, "drop");
        }
    }
}

impl fmt::Display for AddressSpace {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let page_table_root = if let Some(mapping) = &self.mapping {
            mapping.page_table_root().address()
        } else {
            Phys::default()
        };

        match self.kind {
            Kind::Invalid => write!(formatter, "\"invalid\" @ {page_table_root}"),
            Kind::Base => write!(formatter, "\"base\" @ {page_table_root}"),
            Kind::Process => match self.pid {
                Some(pid) => write!(formatter, "\"{pid}\" @ {page_table_root}"),
                None => write!(formatter, "\"process\" @ {page_table_root}"),
            },
        }
    }
}

impl Translate for AddressSpace {
    fn path(
        &mut self,
        virt: Virt,
    ) -> Path<'_> {
        self.mapping().expect("invalid AddressSpace").path(virt)
    }

    fn make_recursive_mapping(&mut self) -> Result<usize> {
        self.mapping()?.make_recursive_mapping()
    }

    #[allow(clippy::needless_arbitrary_self_type)]
    #[duplicate_item(
        method;
        [remove_recursive_mappings];
        [unmap_unused_intermediate];
    )]
    fn method(&mut self) {
        if let Some(mapping) = &mut self.mapping {
            mapping.method();
        }
    }
}

/// Тип виртуального адресного пространства для записи в журнал.
#[derive(Debug, Default)]
enum Kind {
    /// Некорректное виртуальное адресное пространство.
    #[default]
    Invalid,

    /// Базовое виртуальное адресное пространство.
    Base,

    /// Виртуальное адресное пространство некоторого процесса.
    Process,
}

/// Базовое виртуальное адресное пространство.
///
/// Создаётся загрузчиком до старта ядра и донастраивается ядром.
/// Все остальные адресные пространства получаются из него и его копий
/// с помощью метода [`AddressSpace::duplicate()`].
#[allow(rustdoc::private_intra_doc_links)]
pub static BASE_ADDRESS_SPACE: Spinlock<AddressSpace> = Spinlock::new(AddressSpace::zero());

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use duplicate::duplicate_item;

    use crate::error::Result;

    use super::{
        super::{
            Block,
            FrameGuard,
            Page,
            Path,
            Translate,
            Virt,
            mapping::{
                Mapping,
                MappingIterator,
            },
            mmu::{
                PageTable,
                PageTableEntry,
                PageTableFlags,
            },
            page_allocator::test_scaffolding::block,
        },
        AddressSpace,
        Phys2Virt,
    };

    pub unsafe fn map_page(
        address_space: &mut AddressSpace,
        page: Page,
        flags: PageTableFlags,
    ) -> Result<()> {
        unsafe { address_space.map_page(page, flags).map(|_| ()) }
    }

    pub unsafe fn map_page_to_frame(
        address_space: &mut AddressSpace,
        page: Page,
        frame: FrameGuard,
        flags: PageTableFlags,
    ) -> Result<()> {
        unsafe { address_space.map_page_to_frame(page, *frame, flags) }
    }

    pub unsafe fn unmap_page(
        address_space: &mut AddressSpace,
        page: Page,
    ) -> Result<()> {
        unsafe { address_space.unmap_page(page) }
    }

    pub fn check_permission<T>(
        address_space: &mut AddressSpace,
        block: Block<Virt>,
        flags: PageTableFlags,
    ) -> Result<&'static [T]> {
        address_space.check_permission(block, flags)
    }

    pub fn check_permission_mut<T>(
        address_space: &mut AddressSpace,
        block: Block<Virt>,
        flags: PageTableFlags,
    ) -> Result<&'static mut [T]> {
        address_space.check_permission_mut(block, flags)
    }

    pub fn duplicate(address_space: &AddressSpace) -> Result<AddressSpace> {
        address_space.duplicate()
    }

    pub fn iter_mut(address_space: &mut AddressSpace) -> MappingIterator<'_> {
        mapping_mut(address_space).iter_mut()
    }

    pub fn page_allocator_block(address_space: &AddressSpace) -> Block<Page> {
        block(&address_space.user_page_allocator)
    }

    pub fn page_table_root(address_space: &AddressSpace) -> &PageTable {
        let mapping = mapping_ref(address_space);
        unsafe { mapping.page_table_ref(mapping.page_table_root()) }
    }

    pub fn path(
        address_space: &mut AddressSpace,
        virt: Virt,
    ) -> Path<'_> {
        mapping_mut(address_space).path(virt)
    }

    pub fn phys2virt(address_space: &AddressSpace) -> Phys2Virt {
        mapping_ref(address_space).phys2virt()
    }

    pub fn switch_to(address_space: &AddressSpace) {
        address_space.switch_to();
    }

    pub fn translate(
        address_space: &mut AddressSpace,
        virt: Virt,
    ) -> Result<&mut PageTableEntry> {
        address_space.translate(virt)
    }

    pub fn unmap_unused_intermediate(address_space: &mut AddressSpace) {
        address_space.unmap_unused_intermediate();
    }

    #[duplicate_item(
        mapping_getter reference(x) converter;
        [mapping_ref] [&x] [as_ref];
        [mapping_mut] [&mut x] [as_mut];
    )]
    fn mapping_getter(address_space: reference([AddressSpace])) -> reference([Mapping]) {
        address_space.mapping.converter().expect("invalid AddressSpace")
    }
}

