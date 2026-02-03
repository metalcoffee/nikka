/// Аллокатор памяти общего назначения в пространстве пользователя, реализованный через
/// системные вызовы [`syscall::map()`], [`syscall::unmap()`] и [`syscall::copy_mapping()`].
mod map;

use core::{
    alloc::Layout,
    cell::RefCell,
};

use ku::{
    allocator::{
        Cache,
        Clip,
        DetailedInfo,
        Dispatcher,
        FIXED_SIZE_COUNT,
        Info,
    },
    log::debug,
    sync::Spinlock,
};

use map::MapAllocator;

// Used in docs.
#[allow(unused)]
use crate::syscall;

/// Статистика глобального аллокатора памяти общего назначения в пространстве пользователя.
pub fn info() -> Info {
    GLOBAL_ALLOCATOR.info()
}

/// Распечатывает детальную статистику аллокатора.
pub fn dump_info() {
    /// Память под детальную статистику аллокатора.
    static DETAILED_INFO: Spinlock<DetailedInfo> = Spinlock::new(DetailedInfo::new());

    let mut allocator_info = DETAILED_INFO.lock();
    GLOBAL_ALLOCATOR.detailed_info(&mut allocator_info);
    debug!(%allocator_info);
}

/// Обработчик ошибок глобального аллокатора памяти общего назначения.
#[alloc_error_handler]
#[cold]
#[inline(never)]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("failed to allocate memory, layout = {:?}", layout)
}

/// Однопоточный кэш выделяемых блоков памяти для пользовательских приложений.
pub struct SingleThreadedCache(RefCell<[Clip; FIXED_SIZE_COUNT]>);

impl SingleThreadedCache {
    /// Возвращает однопоточный кэш выделяемых блоков памяти для пользовательских приложений.
    pub const fn new() -> Self {
        Self(RefCell::new([const { Clip::new() }; FIXED_SIZE_COUNT]))
    }
}

impl Cache for SingleThreadedCache {
    fn with_borrow_mut<F: FnOnce(&mut Clip) -> R, R>(
        &self,
        index: usize,
        f: F,
    ) -> R {
        let clip =
            &mut self.0.try_borrow_mut().expect("User applications in Nikka are single-threaded")
                [index];
        f(clip)
    }

    const CACHE_AVAILABLE: bool = true;
}

impl Default for SingleThreadedCache {
    fn default() -> Self {
        Self::new()
    }
}

/// [`SingleThreadedCache`] должен использоваться только в однопоточном режиме,
/// так как для пользовательских приложений многопоточность не поддержана.
unsafe impl Send for SingleThreadedCache {
}

/// [`SingleThreadedCache`] должен использоваться только в однопоточном режиме,
/// так как для пользовательских приложений многопоточность не поддержана.
unsafe impl Sync for SingleThreadedCache {
}

/// Глобальный аллокатор памяти общего назначения в пространстве пользователя, реализованный через
/// системные вызовы [`syscall::map()`], [`syscall::unmap()`] и [`syscall::copy_mapping()`].
#[global_allocator]
static GLOBAL_ALLOCATOR: Dispatcher<SingleThreadedCache, MapAllocator> =
    Dispatcher::new(SingleThreadedCache::new(), MapAllocator::new());
