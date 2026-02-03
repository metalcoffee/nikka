use core::{
    mem,
    ptr::NonNull,
};

use derive_more::{
    Deref,
    DerefMut,
};
use heapless::Vec;
use static_assertions::const_assert_eq;

use crate::sync::Spinlock;

use super::{
    FIXED_SIZE_COUNT,
    FixedSizeAllocator,
    Info,
};

#[allow(rustdoc::private_intra_doc_links)]
/// Один элемент кэша.
/// Содержит [`CLIP_SIZE`] ячеек,
/// предназначенных для блоков одинакового фиксированного размера.
///
/// Выровнен на двойной размер линии кэша для производительности, см.
/// [size and alignment](https://docs.rs/crossbeam/latest/crossbeam/utils/struct.CachePadded.html#size-and-alignment)
#[derive(Debug, Deref, DerefMut)]
#[repr(align(128))]
pub struct Clip {
    /// Блоки, которые сохранены в этой ячейке кэша.
    /// Все они должны быть выделены из [`Clip::allocation_owner`].
    #[deref]
    #[deref_mut]
    allocations: Vec<*mut u8, CLIP_SIZE>,

    /// Аллокатор блоков фиксированного размера,
    /// блоки которого сохранены в [`Clip::allocations`].
    allocation_owner: Option<NonNull<Spinlock<FixedSizeAllocator>>>,

    /// Статистика аллокатора, накопленная по операциям с ячейкой кэша.
    info: Info,
}

impl Clip {
    /// Создаёт элемент кэша.
    pub const fn new() -> Self {
        Self {
            allocations: Vec::new(),
            allocation_owner: None,
            info: Info::new(),
        }
    }

    /// Статистика аллокатора, накопленная по операциям с ячейкой кэша.
    pub(super) fn info_mut(&mut self) -> &mut Info {
        &mut self.info
    }

    /// Возвращает `true` если ячейка кэша уже привязана к владельцу блоков,
    /// которые будут в ней храниться.
    /// См. также [`Clip::bind()`] и [`Clip::bind_unchecked()`].
    #[allow(unused)] // It is ok to call [`Clip::bind_unchecked()`] several times with the same `allocation_owner`.
    pub(super) fn is_bound(&self) -> bool {
        self.allocation_owner.is_some()
    }

    /// Привязывает ячейку кэша к владельцу блоков, которые будут в ней храниться.
    /// Эта операция должна быть выполнена до того,
    /// как в ячейку будет сохранён хотя бы один блок.
    ///
    /// Если владелец блоков `allocation_owner` будет уничтожен в тот момент,
    /// пока в ячейке кэша остались его блоки, он запаникует.
    /// Так как в [`FixedSizeAllocator::drop()`] есть проверка, что все блоки
    /// свободны.
    /// А блоки, находящиеся в [`Clip`] помечаются занятыми с точки зрения
    /// [`FixedSizeAllocator`].
    ///
    /// # Panics
    ///
    /// Паникует:
    ///   - Если в ячейка кэша уже есть блоки.
    ///   - Если [`Clip::bind()`] или [`Clip::bind_unchecked()`] не вызывался,
    ///     запаникуют [`Clip::push()`] и [`Clip::drop()`].
    ///     В отладочной сборке также запаникует [`Clip::push_unchecked()`].
    #[allow(unused)] // [`Clip::bind_unchecked()`] can be used instead.
    pub(super) fn bind(
        &mut self,
        allocation_owner: &Spinlock<FixedSizeAllocator>,
    ) {
        assert!(self.is_empty());

        unsafe {
            self.bind_unchecked(allocation_owner);
        }
    }

    /// Привязывает ячейку кэша к владельцу блоков, которые будут в ней храниться.
    /// Эта операция должна быть выполнена до того,
    /// как в ячейку будет сохранён хотя бы один блок.
    ///
    /// Если владелец блоков `allocation_owner` будет уничтожен в тот момент,
    /// пока в ячейке кэша остались его блоки, он запаникует.
    /// Так как в [`FixedSizeAllocator::drop()`] есть проверка, что все блоки
    /// свободны.
    /// А блоки, находящиеся в [`Clip`] помечаются занятыми с точки зрения
    /// [`FixedSizeAllocator`].
    ///
    /// # Panics
    ///
    /// Паникует:
    ///   - В отладочной сборке, если в ячейка кэша уже есть блоки,
    ///     а [`Clip::allocation_owner`] либо не определён, либо не совпадает
    ///     с `allocation_owner`.
    ///   - Если [`Clip::bind()`] или [`Clip::bind_unchecked()`] не вызывался,
    ///     запаникуют [`Clip::push()`] и [`Clip::drop()`].
    ///     В отладочной сборке также запаникует [`Clip::push_unchecked()`].
    ///
    /// # Safety
    ///
    /// В ячейке кэша не должно быть блоков, относящихся к другому [`FixedSizeAllocator`].
    #[allow(unused)] // [`Clip::bind()`] can be used instead.
    pub(super) unsafe fn bind_unchecked(
        &mut self,
        allocation_owner: &Spinlock<FixedSizeAllocator>,
    ) {
        let allocation_owner = Some(allocation_owner.into());
        debug_assert!(self.is_empty() || self.allocation_owner == allocation_owner);

        self.allocation_owner = allocation_owner;
    }

    /// Сохраняет в ячейке кэша блок по указателю `ptr`.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку, содержащую исходный `ptr`, если эта ячейка кэша переполнена.
    ///
    /// # Safety
    ///
    /// Все блоки, сохраняемые в этой ячейке кэша должны относиться к [`FixedSizeAllocator`],
    /// которые привязан через [`Clip::bind()`] или [`Clip::bind_unchecked()`].
    /// В том числе, `ptr` должен быть выделен из `allocation_owner`.
    ///
    /// # Panics
    ///
    /// Паникует, если `allocation_owner` не был привязан через
    /// [`Clip::bind()`] или [`Clip::bind_unchecked()`] ранее
    /// или не совпадает с ранее привязанным.
    #[allow(unused)] // [`Clip::push_unchecked()`] can be used instead.
    #[inline(always)]
    pub(super) fn push(
        &mut self,
        ptr: *mut u8,
    ) -> Result<(), *mut u8> {
        assert!(self.allocation_owner.is_some());

        self.allocations.push(ptr)
    }

    /// Сохраняет в ячейке кэша блок по указателю `ptr`.
    ///
    /// # Safety
    ///
    /// Все блоки, сохраняемые в этой ячейке кэша должны относиться к [`FixedSizeAllocator`],
    /// которые привязан через [`Clip::bind()`] или [`Clip::bind_unchecked()`].
    /// А он должен совпадать с `allocation_owner`.
    /// В том числе, `ptr` должен быть выделен из `allocation_owner`.
    #[allow(unused)] // [`Clip::push()`] can be used instead.
    #[inline(always)]
    pub(super) unsafe fn push_unchecked(
        &mut self,
        ptr: *mut u8,
    ) {
        unsafe {
            self.allocations.push_unchecked(ptr);
        }
    }
}

impl Default for Clip {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        if let Some(mut allocation_owner) = self.allocation_owner &&
            !self.allocations.is_empty()
        {
            let allocation_owner = unsafe { allocation_owner.as_mut() };
            allocation_owner.lock().unfill_clip(self, 0);
        }

        assert!(self.allocations.is_empty());

        self.allocation_owner = None;
    }
}

/// Типаж кэша выделяемых блоков памяти.
pub trait Cache {
    /// Выполняет операцию `F` над элементом кэша выделяемых блоков памяти с индексом `index`.
    /// Возвращает результат операции `F`.
    ///
    /// # Panics
    ///
    /// Паникует, если кэш не реализован, то есть если
    /// [`Cache::CACHE_AVAILABLE`] равен `false`.
    fn with_borrow_mut<F: FnOnce(&mut Clip) -> R, R>(
        &self,
        index: usize,
        f: F,
    ) -> R;

    /// Признак существования кэша.
    /// Равен `true` если кэш реализован и им можно пользоваться.
    const CACHE_AVAILABLE: bool;
}

/// Единый кэш выделяемых блоков памяти на все потоки.
/// Используется, если не доступен
/// [TLS](https://en.wikipedia.org/wiki/Thread-local_storage).
pub struct GlobalCache([Spinlock<Clip>; FIXED_SIZE_COUNT]);

impl GlobalCache {
    /// Создаёт единый кэш выделяемых блоков памяти на все потоки.
    pub const fn new() -> Self {
        Self([const { Spinlock::new(Clip::new()) }; FIXED_SIZE_COUNT])
    }
}

impl Default for GlobalCache {
    fn default() -> Self {
        Self::new()
    }
}

/// См. [The Rustonomicon, "Send and Sync"](https://doc.rust-lang.org/nomicon/send-and-sync.html).
///
/// `*mut u8` в [`Clip`] препятствует автоматическому выводу этого типажа.
/// Поэтому мы вручную должны поддерживать такой инвариант ---
/// поток, который помещает этот указатель в [`Clip`] не оставляет себе его копий.
unsafe impl Sync for GlobalCache {
}

impl Cache for GlobalCache {
    fn with_borrow_mut<F: FnOnce(&mut Clip) -> R, R>(
        &self,
        index: usize,
        f: F,
    ) -> R {
        let mut clip = self.0[index].lock();
        f(&mut clip)
    }

    const CACHE_AVAILABLE: bool = true;
}

/// Количество блоков каждого фиксированного размера в кэше выделяемых блоков памяти.
pub(super) const CLIP_SIZE: usize = 32 - CLIP_METADATA_SIZE.div_ceil(mem::size_of::<*mut u8>());

/// Размер [`Clip`] без учёта кэшируемых указателей.
const CLIP_METADATA_SIZE: usize = mem::size_of::<Vec<*mut u8, 0>>() +
    mem::size_of::<Option<NonNull<Spinlock<FixedSizeAllocator>>>>() +
    mem::size_of::<Info>();

/// Двойной размер линии кэша, см.
/// [size and alignment](https://docs.rs/crossbeam/latest/crossbeam/utils/struct.CachePadded.html#size-and-alignment)
const CACHE_LINE_SIZE: usize = 128;

const_assert_eq!(mem::size_of::<Clip>() % CACHE_LINE_SIZE, 0);

// TODO: your code here.
