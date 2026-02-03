use core::ops::{
    Deref,
    DerefMut,
};

use x86_64::instructions::interrupts;

use super::{
    PanicStrategy,
    Spinlock,
    SpinlockGuard,
};

/// Спин-блокировка, которая позволяет синхронизировать доступ
/// к защищаемым ею данным как из обычного кода, так и из обработчика прерываний.
/// В остальном аналогична [`Spinlock`].
pub struct IrqSpinlock<T, const PANIC_STRATEGY: PanicStrategy = { PanicStrategy::Halt }>(
    Spinlock<T, PANIC_STRATEGY>,
);

impl<T, const PANIC_STRATEGY: PanicStrategy> IrqSpinlock<T, PANIC_STRATEGY> {
    /// Создаёт новую спин-блокировку для защиты `data`.
    pub const fn new(data: T) -> Self {
        Self(Spinlock::new(data))
    }

    // ANCHOR: lock
    /// Захватывает спин-блокировку.
    /// При этом ожидает в активном цикле освобождения блокировки, если она уже захвачена.
    ///
    /// Возвращает [`IrqSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`IrqSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    pub fn lock(&self) -> IrqSpinlockGuard<'_, T, PANIC_STRATEGY> {
        let irq_guard = IrqGuard::new();

        IrqSpinlockGuard {
            spinlock_guard: self.0.lock(),
            irq_guard,
        }
    }
    // ANCHOR_END: lock

    // ANCHOR: try_lock
    /// Пытается захватить спин-блокировку.
    /// Если она уже захвачена, возвращает [`None`].
    ///
    /// Если спин-блокировка свободна и нет конкурирующих за неё потоков,
    /// гарантированно захватывает её.
    ///
    /// При успехе возвращает [`IrqSpinlockGuard`], который:
    ///   - Позволяет читать и писать в защищаемые [`IrqSpinlock`] данные
    ///     с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
    ///   - Автоматически освобождает блокировку в реализации типажа [`Drop`].
    pub fn try_lock(&self) -> Option<IrqSpinlockGuard<'_, T, PANIC_STRATEGY>> {
        let irq_guard = IrqGuard::new();

        self.0.try_lock().map(|spinlock_guard| IrqSpinlockGuard {
            spinlock_guard,
            irq_guard,
        })
    }
    // ANCHOR_END: try_lock
}

impl<T, const PANIC_STRATEGY: PanicStrategy> Deref for IrqSpinlock<T, PANIC_STRATEGY> {
    type Target = Spinlock<T, PANIC_STRATEGY>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> DerefMut for IrqSpinlock<T, PANIC_STRATEGY> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Вспомогательная структура для [`IrqSpinlock`].
///
/// - Запоминает состояние флага разрешения прерываний в момент создания.
/// - После чего запрещает прерывания.
/// - Автоматически возвращает флаг разрешения прерываний
///   в исходное состояние в реализации типажа [`Drop`].
struct IrqGuard {
    were_enabled: bool,
}

impl IrqGuard {
    /// Создаёт [`IrqGuard`].
    ///
    /// - Запоминает состояние флага разрешения прерываний в момент создания.
    /// - После чего запрещает прерывания.
    fn new() -> Self {
        let were_enabled = interrupts::are_enabled();
        interrupts::disable();
        Self { were_enabled }
    }
}

impl Drop for IrqGuard {
    /// Вызывается при разрушении [`IrqGuard`].
    /// Возвращает флаг разрешения прерываний в исходное состояние,
    /// в котором он находился до создания этого [`IrqGuard`].
    fn drop(&mut self) {
        if self.were_enabled {
            interrupts::enable();
        }
    }
}

#[allow(rustdoc::private_intra_doc_links)]
/// Захваченный на запись [`IrqSpinlock`].
///
/// - Позволяет читать и писать в защищаемые [`IrqSpinlock`] данные
///   с помощью типажей [`Deref`] и [`DerefMut`] соответственно.
/// - В реализации типажа [`Drop`]:
///   - Автоматически освобождает блокировку.
///   - После этого, возвращает флаг разрешения прерываний в исходное состояние,
///     в котором он находился до создания этого [`IrqGuard`].
pub struct IrqSpinlockGuard<'a, T, const PANIC_STRATEGY: PanicStrategy = { PanicStrategy::Halt }> {
    /// Захваченный на запись [`Spinlock`].
    spinlock_guard: SpinlockGuard<'a, T, PANIC_STRATEGY>,

    /// Должен быть после [`IrqSpinlockGuard::spinlock_guard`],
    /// чтобы в при разрушении [`IrqSpinlockGuard`] сначала разрушился
    /// [`IrqSpinlockGuard::spinlock_guard`] и освободилась спин-блокировка.
    /// А уже потом --- разрушился [`IrqSpinlockGuard::irq_guard`] и
    /// включились прерывания.
    // Используется только метод [`IrqGuard::drop()`], это происходит неявно.
    #[allow(dead_code)]
    irq_guard: IrqGuard,
}

impl<'a, T, const PANIC_STRATEGY: PanicStrategy> Deref for IrqSpinlockGuard<'a, T, PANIC_STRATEGY> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.spinlock_guard.deref()
    }
}

impl<T, const PANIC_STRATEGY: PanicStrategy> DerefMut for IrqSpinlockGuard<'_, T, PANIC_STRATEGY> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.spinlock_guard.deref_mut()
    }
}
