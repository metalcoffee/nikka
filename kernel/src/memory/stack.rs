use core::{
    mem,
    ptr,
};

use lazy_static::lazy_static;

use ku::sync::spinlock::Spinlock;

use crate::error::Result;

use super::{
    AddressSpace,
    BASE_ADDRESS_SPACE,
    Block,
    Page,
    Virt,
    mmu::PageTableFlags,
};

// Used in docs.
#[allow(unused)]
use crate as kernel;

/// Выровненный на границу страницы стек.
#[repr(C, align(4096))]
pub(crate) struct Stack([u8; Self::STACK_SIZE]);

impl Stack {
    /// Создаёт статически выделенный стек.
    const fn new_static() -> Self {
        Self([0; mem::size_of::<Self>()])
    }

    /// Выделяет стек с флагами доступа `flags` в адресном пространстве `address_space`.
    pub(crate) fn new(
        address_space: &mut AddressSpace,
        flags: PageTableFlags,
    ) -> Result<&'static mut Self> {
        let flags = flags & !PageTableFlags::EXECUTABLE;
        Ok(&mut Self::new_slice(address_space, flags, 1)?[0])
    }

    /// Выделяет `len` стеков с флагами доступа `flags` в адресном пространстве `address_space`.
    pub(crate) fn new_slice(
        address_space: &mut AddressSpace,
        flags: PageTableFlags,
        len: usize,
    ) -> Result<&'static mut [Self]> {
        let flags = flags & !PageTableFlags::EXECUTABLE;
        let stacks = unsafe { address_space.map_slice_zeroed::<Self>(len, flags)? };

        for stack in stacks.iter_mut() {
            unsafe {
                stack.make_guard_zone(address_space)?;
            }
        }

        Ok(stacks)
    }

    /// Возвращает указатель на вершину пустого стека.
    pub(crate) fn pointer(&self) -> Virt {
        Virt::from_ptr(self.0.as_ptr_range().end)
    }

    /// Возвращает разбивку стека на защитный блок памяти и блок для данных в стеке.
    pub(crate) fn zones(&self) -> (Block<Virt>, Block<Virt>) {
        let zones = &self.0.split_at(Self::GUARD_ZONE_SIZE);

        (Block::from_slice(zones.0), Block::from_slice(zones.1))
    }

    /// Создаёт в стеке не отображённый блок памяти,
    /// защищающий от неопределённого поведения при переполнении стека.
    unsafe fn make_guard_zone(
        &mut self,
        address_space: &mut AddressSpace,
    ) -> Result<()> {
        unsafe { address_space.unmap_slice(&mut self.0[.. Self::GUARD_ZONE_SIZE]) }
    }

    /// Размер не отображённой в память защитной области стека.
    /// Которая служит для обнаружения переполнения стека и
    /// не допускает перезапись других данных в этом случае.
    pub(super) const GUARD_ZONE_SIZE: usize = Page::SIZE;

    /// Размер стеков, включая не отображённую в память защитную область.
    const STACK_SIZE: usize = 32 * Page::SIZE;
}

/// Создаёт статически выделенный стек.
macro_rules! make_static_stack {
    () => {{
        #[unsafe(link_section = ".stack")]
        static mut STACK: Stack = Stack::new_static();

        Virt::from_ptr(ptr::addr_of_mut!(STACK))
    }};
}

/// Выделенные стеки для непредвиденных исключений.
pub(crate) struct ExceptionStacks {
    /// Стек, который используется всеми процессорами в случае наступления
    /// [Double Fault](https://en.wikipedia.org/wiki/Double_fault).
    /// То есть в аварийной ситуации, когда
    /// [продолжение миссии невозможно](https://en.wikipedia.org/wiki/Launch_escape_system).
    /// ![](https://upload.wikimedia.org/wikipedia/commons/thumb/1/17/Apollo_Pad_Abort_Test_-2.jpg/440px-Apollo_Pad_Abort_Test_-2.jpg)
    launch_escape_stack: Virt,

    /// Стек для обработки [Page Fault](https://en.wikipedia.org/wiki/Page_fault) на
    /// Bootstrap Processor до инициализации структур [`kernel::smp::Cpu`].
    page_fault_stack: Virt,
}

impl ExceptionStacks {
    /// Создаёт статические стеки для непредвиденных исключений.
    fn new() -> Self {
        Self {
            launch_escape_stack: make_static_stack!(),
            page_fault_stack: make_static_stack!(),
        }
    }

    /// После инициализации системы виртуальной памяти
    /// добавляет в стеки для непредвиденных исключений защитные зоны.
    pub(crate) fn make_guard_zones(&mut self) -> Result<()> {
        let address_space = &mut BASE_ADDRESS_SPACE.lock();

        for stack in [self.launch_escape_stack, self.page_fault_stack] {
            unsafe {
                stack
                    .try_into_mut::<Stack>()
                    .expect(Self::MESSAGE)
                    .make_guard_zone(address_space)?;
            }
        }

        Ok(())
    }

    /// Возвращает указатель на вершину пустого стека выделенного для обработки
    /// [Double Fault](https://en.wikipedia.org/wiki/Double_fault).
    pub(crate) fn double_fault_rsp(&self) -> Virt {
        unsafe { self.launch_escape_stack.try_into_ref::<Stack>().expect(Self::MESSAGE).pointer() }
    }

    /// Возвращает указатель на вершину пустого стека выделенного для обработки
    /// [Page Fault](https://en.wikipedia.org/wiki/Page_fault).
    pub(super) fn page_fault_rsp(&self) -> Virt {
        unsafe { self.page_fault_stack.try_into_ref::<Stack>().expect(Self::MESSAGE).pointer() }
    }

    /// Адрес стека нулевой или не выровнен по границе страниц.
    const MESSAGE: &str = "exception stacks are not initialized properly";
}

lazy_static! {
    /// Выделенные стеки для непредвиденных исключений.
    pub(crate) static ref EXCEPTION_STACKS: Spinlock<ExceptionStacks> =
        Spinlock::new(ExceptionStacks::new());
}
