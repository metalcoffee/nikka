use core::{
    marker::ConstParamTy,
    sync::atomic::{
        AtomicBool,
        Ordering,
    },
};

/// Стратегия поведения блокировки в случае, если возникла паника.
/// Аналогична
/// [Mutex poisoning](https://doc.rust-lang.org/std/sync/struct.Mutex.html#poisoning).
#[derive(Clone, ConstParamTy, Copy, Debug, PartialEq, Eq)]
pub enum PanicStrategy {
    /// Попытка захвата блокировки принудительно останавливает процессор.
    Halt,

    /// Блокировка допускает произвольное количество конкурирующих захватов в случае паники.
    /// Используется для журналирования.
    KnockDown,
}

/// Включает для блокировок поведение при панике.
pub fn start_panicking() {
    PANIC.store(true, Ordering::Relaxed);
}

/// Возвращает `true` если для блокировок включено поведение при панике.
pub(super) fn is_panicking() -> bool {
    PANIC.load(Ordering::Relaxed)
}

/// Содержит `true` если для блокировок включено поведение при панике.
static PANIC: AtomicBool = AtomicBool::new(false);
