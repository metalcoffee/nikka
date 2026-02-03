use core::{
    cell::UnsafeCell,
    fmt,
    hint,
    ops::{
        Deref,
        DerefMut,
    },
    panic::Location,
    sync::atomic::{
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
};

use crate::{
    backtrace::Callsite,
    log::trace,
};

use super::{
    PanicStrategy,
    panic,
    sequence_lock::SequenceLock,
};

/// Спин-блокировка, которая позволяет синхронизировать доступ
/// к защищаемым ею данным из разных потоков исполнения.
///
/// <https://en.wikipedia.org/wiki/Spinlock>
///
/// Для избежания
/// [ложного совместного использования](https://en.wikipedia.org/wiki/False_sharing)
/// выровнена на размер линии кэша.
/// Точнее, на её
/// [удвоенный размер](https://docs.rs/crossbeam/latest/crossbeam/utils/struct.CachePadded.html#size-and-alignment).
///
/// # Examples
///
/// ## Захват спин-блокировки, использование защищаемых данных и неявное освобождение спин-блокировки
/// ```rust
/// # use ku::sync::spinlock::Spinlock;
/// #
/// // Инициализация спин-блокировки с одновременной инициализацией защищаемых ею данных.
/// let spinlock: Spinlock<i32> = Spinlock::new(42);
///
/// {
///     // Захват спин-блокировки, который возвращает объект типа SpinlockGuard.
///     let mut lock = spinlock.lock();
///
///     // Через этот объект можно получить доступ к защищаемым данным.
///     *lock += 1;
///
///     // При завершении области видимости переменной lock типа SpinlockGuard
///     // происходит неявное освобождение спин-блокировки.
/// }
/// #
/// # assert!(spinlock.try_lock().is_some());
/// ```
///
/// ## Явное освобождение блокировки
/// ```rust
/// # use ku::sync::spinlock::Spinlock;
/// #
/// // Инициализация спин-блокировки с одновременной инициализацией защищаемых ею данных.
/// let spinlock: Spinlock<i32> = Spinlock::new(42);
///
/// // Захват спин-блокировки, который возвращает объект типа SpinlockGuard.
/// let mut lock = spinlock.lock();
///
/// // Через этот объект можно получить доступ к защищаемым данным.
/// *lock += 1;
///
/// // При разрушении переменной lock типа SpinlockGuard с помощью функции mem::drop()
/// // происходит явное освобождение спин-блокировки.
/// drop(lock);
/// #
/// # assert!(spinlock.try_lock().is_some());
/// ```
///
/// ## Попытка захвата блокировки
/// ```rust
/// # use ku::sync::spinlock::Spinlock;
/// #
/// // Инициализация спин-блокировки с одновременной инициализацией защищаемых ею данных.
/// let spinlock: Spinlock<i32> = Spinlock::new(42);
///
/// if let Some(mut lock) = spinlock.try_lock() {
///     // Удачная попытка захвата спин-блокировки.
///
///     // Через этот объект можно получить доступ к защищаемым данным.
///     *lock += 1;
///
///     // Повторная попытка захвата спин-блокировки обречена на провал.
///     assert!(spinlock.try_lock().is_none());
///
///     // При завершении области видимости переменной lock типа SpinlockGuard
///     // происходит неявное освобождение спин-блокировки.
/// } else {
///     // Блокировку захватить не удалось, переходим к плану Б...
/// }
/// #
/// # assert!(spinlock.try_lock().is_some());
/// ```
///
/// ## Если блокировка находится в эксклюзивном доступе, возможно обращение к данным без захвата блокировки
/// ```rust
/// # use ku::sync::spinlock::Spinlock;
/// #
/// fn f(spinlock: &mut Spinlock<i32>) {
///     // Этот поток исполнения владеет переменной spinlock эксклюзивно,
///     // поэтому конкурентного доступа к ней быть не может.
///     // Borrow checker может доказать это статическим анализом кода.
///     // Так как у нас в распоряжении изменяемая ссылка,
///     // которая гарантированно является уникальной ссылкой на spinlock.
///     // Поэтому Rust позволит обратиться к защищаемым данным без захвата блокировки.
///     *spinlock.get_mut() += 1;
/// }
///
/// // Инициализация спин-блокировки с одновременной инициализацией защищаемых ею данных.
/// let mut spinlock = Spinlock::new(42_i32);
///
/// f(&mut spinlock);
/// ```
#[cfg(not(feature = "benchmark"))]
pub use DebugSpinlock as Spinlock;
#[cfg(feature = "benchmark")]
pub use FastSpinlock as Spinlock;

#[cfg(not(feature = "benchmark"))]
pub use DebugSpinlockGuard as SpinlockGuard;
#[cfg(feature = "benchmark")]
pub use FastSpinlockGuard as SpinlockGuard;

/// Реализация спин-блокировки с отладкой взаимоблокировок и сбором статистики.
/// Подходит для отладки, но работает относительно медленно и
/// поэтому не подходит для профилирования производительности.
#[repr(align(128))]
pub struct DebugSpinlock<T, const PANIC_STRATEGY: PanicStrategy = { PanicStrategy::Halt }> {
    /// Данные, защищаемые спин-блокировкой.
    // TODO: your code here.
    data: FastSpinlock<T, PANIC_STRATEGY>, // TODO: remove before flight.
    /// Место кода, в котором определена переменная спин-блокировки.
    /// Используется для отладочной печати.
    defined: &'static Location<'static>,

    /// Атомарная переменная, сигнализирующая, что спин-блокировка захвачена.
    locked: AtomicBool,

    /// Последний владелец спин-блокировки.
    owner: SequenceLock<Callsite>,

    /// Статистика попыток захвата спин-блокировки.
    stats: Stats,
}

impl<T, const PANIC_STRATEGY: PanicStrategy> DebugSpinlock<T, PANIC_STRATEGY> {
    /// Создаёт новую спин-блокировку для защиты `data`.
    #[track_caller]
    pub const fn new(data: T) -> Self {
        Self {
            // TODO: your code here.
            data: FastSpinlock::new(data), // TODO: remove before flight.
            defined: Location::caller(),
            locked: AtomicBool::new(false),
            owner: SequenceLock::new(Callsite::zero()),
            stats: Stats::new(),
        }
    }

    /// Захватывает спин-блокировку.
    /// При этом ожидает в активном цикле освобождения блокировки, если она уже захвачена.
    ///
    /// Возвращает [`DebugSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`DebugSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    #[track_caller]
    pub fn lock(&self) -> DebugSpinlockGuard<'_, T, PANIC_STRATEGY> {
        if panic::is_panicking() {
            return self.panic_lock();
        }

        // TODO: your code here.
        DebugSpinlockGuard { spinlock: self.data.lock() } // TODO: remove before flight.
    }

    /// Пытается захватить спин-блокировку.
    /// Если она уже захвачена, возвращает [`None`].
    ///
    /// Если спин-блокировка свободна и нет конкурирующих за неё потоков,
    /// гарантированно захватывает её.
    ///
    /// При успехе возвращает [`DebugSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`DebugSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    #[track_caller]
    pub fn try_lock(&self) -> Option<DebugSpinlockGuard<'_, T, PANIC_STRATEGY>> {
        // TODO: your code here.
        Some(DebugSpinlockGuard { spinlock: self.data.try_lock()? }) // TODO: remove before flight.
    }

    /// Пытается захватить спин-блокировку максимум `max_tries` раз.
    /// Если за это количество попыток блокировка не освободилась, возвращает [`None`].
    /// В случае успеха сохраняет `callsite` с метаданными нового владельца спин-блокировки.
    ///
    /// При успехе возвращает [`DebugSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`DebugSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    fn try_lock_impl(
        &self,
        max_tries: usize,
        callsite: Callsite,
    ) -> Option<DebugSpinlockGuard<'_, T, PANIC_STRATEGY>> {
        if panic::is_panicking() {
            return Some(self.panic_lock());
        }

        // TODO: your code here.
        unimplemented!();
    }

    /// Позволяет читать и писать в защищаемые [`DebugSpinlock`] данные без блокирования в случае,
    /// если вызывающий код эксклюзивно владеет [`DebugSpinlock`] --- `&mut self`.
    /// То есть, в случае когда конкурентного доступа к [`DebugSpinlock`] быть не может.
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    /// В зависимости от параметра `PANIC_STRATEGY`:
    ///   - [`PanicStrategy::Halt`] --- останавливает исполнение потока.
    ///   - [`PanicStrategy::KnockDown`] --- принудительно захватывает спин-блокировку,
    ///     даже если она уже захвачена.
    fn panic_lock(&self) -> DebugSpinlockGuard<'_, T, PANIC_STRATEGY> {
        match PANIC_STRATEGY {
            PanicStrategy::Halt => unsafe { crate::halt() },
            PanicStrategy::KnockDown => {
                // TODO: your code here.
                DebugSpinlockGuard { spinlock: self.data.lock() } // TODO: remove before flight.
            },
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for DebugSpinlock<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "Spinlock {{ defined: {}, ", self.defined)?;

        // TODO: your code here.

        write!(formatter, ", stats: {:?} }}", self.stats)
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> Drop for DebugSpinlock<T, PANIC_STRATEGY> {
    fn drop(&mut self) {
        assert_eq!(
            self.stats.locks.load(Ordering::Relaxed),
            self.stats.unlocks.load(Ordering::Relaxed),
        );
        trace!(spinlock = %self.defined, stats = ?self.stats, "dropping");
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> From<T> for DebugSpinlock<T, PANIC_STRATEGY> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

/// См. [The Rustonomicon, "Send and Sync"](https://doc.rust-lang.org/nomicon/send-and-sync.html).
unsafe impl<T: Send, const PANIC_STRATEGY: PanicStrategy> Send
    for DebugSpinlock<T, PANIC_STRATEGY>
{
}

/// См. [The Rustonomicon, "Send and Sync"](https://doc.rust-lang.org/nomicon/send-and-sync.html).
unsafe impl<T: Send, const PANIC_STRATEGY: PanicStrategy> Sync
    for DebugSpinlock<T, PANIC_STRATEGY>
{
}

/// Захваченный на запись [`DebugSpinlock`].
///
/// - Позволяет читать и писать в защищаемые [`DebugSpinlock`] данные
///   с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
/// - Автоматически освобождает блокировку в реализации типажа [`Drop`].
pub struct DebugSpinlockGuard<'a, T, const PANIC_STRATEGY: PanicStrategy = { PanicStrategy::Halt }>
{
    /// Захваченный на запись [`DebugSpinlock`].
    // TODO: your code here.
    spinlock: FastSpinlockGuard<'a, T, PANIC_STRATEGY>, // TODO: remove before flight.
}

impl<T, const PANIC_STRATEGY: PanicStrategy> Deref for DebugSpinlockGuard<'_, T, PANIC_STRATEGY> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // TODO: your code here.
        self.spinlock.deref() // TODO: remove before flight.
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> DerefMut
    for DebugSpinlockGuard<'_, T, PANIC_STRATEGY>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        // TODO: your code here.
        self.spinlock.deref_mut() // TODO: remove before flight.
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> Drop for DebugSpinlockGuard<'_, T, PANIC_STRATEGY> {
    fn drop(&mut self) {
        // TODO: your code here.
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> fmt::Debug
    for DebugSpinlockGuard<'_, T, PANIC_STRATEGY>
where
    T: fmt::Debug,
{
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{:?}", self.deref())
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> fmt::Display
    for DebugSpinlockGuard<'_, T, PANIC_STRATEGY>
where
    T: fmt::Display,
{
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}", self.deref())
    }
}

/// Быстрая реализация спин-блокировки без дополнительных возможностей.
/// Подходит для профилирования производительности.
#[repr(align(128))]
pub struct FastSpinlock<T, const PANIC_STRATEGY: PanicStrategy = { PanicStrategy::Halt }> {
    /// Данные, защищаемые спин-блокировкой.
    data: spin::Mutex<T>,
}

impl<T, const PANIC_STRATEGY: PanicStrategy> FastSpinlock<T, PANIC_STRATEGY> {
    /// Создаёт новую спин-блокировку для защиты `data`.
    pub const fn new(data: T) -> Self {
        Self {
            data: spin::Mutex::new(data),
        }
    }

    /// Захватывает спин-блокировку.
    /// При этом ожидает в активном цикле освобождения блокировки, если она уже захвачена.
    ///
    /// Возвращает [`FastSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`FastSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    pub fn lock(&self) -> FastSpinlockGuard<'_, T, PANIC_STRATEGY> {
        if panic::is_panicking() {
            return self.panic_lock();
        }

        FastSpinlockGuard {
            spinlock: self.data.lock(),
        }
    }

    /// Пытается захватить спин-блокировку.
    /// Если она уже захвачена, возвращает [`None`].
    ///
    /// Если спин-блокировка свободна и нет конкурирующих за неё потоков,
    /// гарантированно захватывает её.
    ///
    /// При успехе возвращает [`FastSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`FastSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    pub fn try_lock(&self) -> Option<FastSpinlockGuard<'_, T, PANIC_STRATEGY>> {
        if panic::is_panicking() {
            return Some(self.panic_lock());
        }

        Some(FastSpinlockGuard {
            spinlock: self.data.try_lock()?,
        })
    }

    /// Позволяет читать и писать в защищаемые [`FastSpinlock`] данные без блокирования в случае,
    /// если вызывающий код эксклюзивно владеет [`FastSpinlock`] --- `&mut self`.
    /// То есть, в случае когда конкурентного доступа к [`FastSpinlock`] быть не может.
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    /// В зависимости от параметра `PANIC_STRATEGY`:
    ///   - [`PanicStrategy::Halt`] --- останавливает исполнение потока.
    ///   - [`PanicStrategy::KnockDown`] --- принудительно захватывает спин-блокировку,
    ///     даже если она уже захвачена.
    fn panic_lock(&self) -> FastSpinlockGuard<'_, T, PANIC_STRATEGY> {
        match PANIC_STRATEGY {
            PanicStrategy::Halt => unsafe { crate::halt() },
            PanicStrategy::KnockDown => loop {
                if let Some(guard) = self.data.try_lock() {
                    return FastSpinlockGuard { spinlock: guard };
                }

                unsafe {
                    self.data.force_unlock();
                }
            },
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for FastSpinlock<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{:?}", self.data)
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> From<T> for FastSpinlock<T, PANIC_STRATEGY> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

/// Захваченный на запись [`FastSpinlock`].
///
/// - Позволяет читать и писать в защищаемые [`FastSpinlock`] данные
///   с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
/// - Автоматически освобождает блокировку в реализации типажа [`Drop`].
pub struct FastSpinlockGuard<'a, T, const PANIC_STRATEGY: PanicStrategy = { PanicStrategy::Halt }> {
    /// Захваченный на запись [`FastSpinlock`].
    spinlock: spin::MutexGuard<'a, T>,
}

impl<T, const PANIC_STRATEGY: PanicStrategy> Deref for FastSpinlockGuard<'_, T, PANIC_STRATEGY> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.spinlock.deref()
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> DerefMut for FastSpinlockGuard<'_, T, PANIC_STRATEGY> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.spinlock.deref_mut()
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> fmt::Debug for FastSpinlockGuard<'_, T, PANIC_STRATEGY>
where
    T: fmt::Debug,
{
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{:?}", self.deref())
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> fmt::Display
    for FastSpinlockGuard<'_, T, PANIC_STRATEGY>
where
    T: fmt::Display,
{
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}", self.deref())
    }
}

/// Статистика захватов и ожиданий освобождения [`Spinlock`].
#[derive(Debug)]
struct Stats {
    /// Количество неуспешных попыток захватить [`Spinlock`].
    failures: AtomicUsize,

    /// Количество успешных попыток захватить [`Spinlock`] --- заходов в критическую секцию.
    locks: AtomicUsize,

    /// Количество отпусканий [`Spinlock`] --- выходов из критической секции.
    unlocks: AtomicUsize,

    /// Количество итераций цикла ожидания на уже захваченном [`Spinlock`].
    /// Если [`Spinlock`] был захвачен на первой же итерации, то есть был свободен,
    /// то `waits` не увеличивается.
    waits: AtomicUsize,
}

impl Stats {
    /// Инициализирует статистику захватов и ожиданий освобождения [`Spinlock`].
    const fn new() -> Self {
        Self {
            failures: AtomicUsize::new(0),
            locks: AtomicUsize::new(0),
            unlocks: AtomicUsize::new(0),
            waits: AtomicUsize::new(0),
        }
    }
}
