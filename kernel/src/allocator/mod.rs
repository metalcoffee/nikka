/// Аллокатор памяти, предназначенный для выделения памяти блоками виртуальных страниц.
mod big;

/// Пара аллокаторов, связанная с одним или с двумя разными адресными пространствами.
/// Реализует типаж [`BigAllocatorPair`].
/// В частности, позволяет скопировать
/// отображение страниц из текущего адресного пространства [`BigPair::src()`]
/// в целевое адресное пространство [`BigPair::dst()`] методом [`BigPair::copy_mapping()`].
mod big_pair;

/// Аллокатор памяти общего назначения внутри [`AddressSpace`].
mod memory_allocator;

use ku::{
    allocator::{
        DetailedInfo,
        Dispatcher,
        GlobalCache,
        Info,
    },
    log::debug,
    sync::Spinlock,
};

use crate::memory::{
    BASE_ADDRESS_SPACE,
    KERNEL_RW,
};

pub(crate) use big::Big;
pub(crate) use big_pair::BigPair;
pub(crate) use memory_allocator::MemoryAllocator;

// Used in docs.
#[allow(unused)]
use {
    crate::memory::AddressSpace,
    ku::allocator::BigAllocatorPair,
};

/// Статистика глобального аллокатора памяти общего назначения для ядра.
pub fn info() -> Info {
    GLOBAL_ALLOCATOR.info()
}

pub(crate) fn pages_allocation(pages: usize) {
    if Info::IS_SUPPORTED {
        GLOBAL_ALLOCATOR.pages_allocation(pages);
    }
}

pub(crate) fn pages_deallocation(pages: usize) {
    if Info::IS_SUPPORTED {
        GLOBAL_ALLOCATOR.pages_deallocation(pages);
    }
}

/// Распечатывает детальную статистику аллокатора.
pub fn dump_info() {
    /// Память под детальную статистику аллокатора.
    static DETAILED_INFO: Spinlock<DetailedInfo> = Spinlock::new(DetailedInfo::new());

    let mut allocator_info = DETAILED_INFO.lock();
    GLOBAL_ALLOCATOR.detailed_info(&mut allocator_info);
    debug!(%allocator_info);
}

/// Обработчик ошибок выделения памяти.
#[alloc_error_handler]
#[cold]
#[inline(never)]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("failed to allocate memory, layout = {:?}", layout)
}

/// Глобальный аллокатор памяти общего назначения для ядра.
/// Выделяет память и отображает её внутри [`BASE_ADDRESS_SPACE`].
#[global_allocator]
static GLOBAL_ALLOCATOR: Dispatcher<GlobalCache, MemoryAllocator<'static>> = Dispatcher::new(
    GlobalCache::new(),
    MemoryAllocator::new(&BASE_ADDRESS_SPACE, KERNEL_RW),
);
