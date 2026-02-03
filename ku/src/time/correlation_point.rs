#![forbid(unsafe_code)]

use core::{
    hint,
    sync::atomic::{
        AtomicI64,
        AtomicU64,
        Ordering,
    },
};

use super::tsc;

/// Предназначена для привязки тактов процессора к другим часам в один момент времени.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CorrelationPoint {
    /// Значение счётчика тиков некоторого источника времени.
    count: i64,

    /// Значение счётчика тактов процессора в тот же момент.
    tsc: i64,
}

impl CorrelationPoint {
    /// Возвращает [`CorrelationPoint`], который соответствует тику `count`,
    /// и привязан к такту `tsc` процессора.
    fn new(
        count: i64,
        tsc: i64,
    ) -> Self {
        assert!(count >= 0);
        assert!(tsc >= 0);

        Self { count, tsc }
    }

    /// Возвращает [`CorrelationPoint`], который соответствует тику `count` и
    /// привязан к текущему такту процессора.
    pub fn now(count: i64) -> Self {
        Self::new(count, tsc::tsc())
    }

    /// Возвращает [`CorrelationPoint`], который соответствует тику `count`,
    /// но не привязан ни к какому такту процессора.
    ///
    /// Используется, когда значение структуры относится не к самому тику `count`,
    /// а к промежутку после него.
    /// И может быть использовано как текущее время с низким разрешением ---
    /// частотой соответствующих часов.
    pub fn invalid(count: i64) -> Self {
        Self::new(count, 0)
    }

    /// Возвращает `true`, если `self` привязан к какому-нибудь такту процессора.
    pub fn is_valid(&self) -> bool {
        self.tsc != 0
    }

    /// Значение счётчика тиков источника времени.
    pub fn count(&self) -> i64 {
        self.count
    }

    /// Значение счётчика тактов процессора.
    pub fn tsc(&self) -> i64 {
        self.tsc
    }
}

/// Предназначена для конкурентного доступа к значениям [`CorrelationPoint`].
///
/// То есть [`CorrelationPoint`] и [`AtomicCorrelationPoint`] соотносятся также как
/// примитивный тип [`i64`] и атомарный [`AtomicI64`].
/// Атомарность нужна для того, чтобы конкурентно
///   - в обработчике прерывания обновлять счётчики [`AtomicCorrelationPoint`];
///   - а в обычном коде читать эти счётчики, чтобы "посмотреть на часы".
///
/// Реализует [неблокирующую синхронизацию](https://en.wikipedia.org/wiki/Non-blocking_algorithm)
/// для согласованного доступа к полям [`AtomicCorrelationPoint`].
/// Использует упрощённый [sequence lock](https://en.wikipedia.org/wiki/Seqlock).
///
/// См. также:
///   - [Writing a seqlock in Rust.](https://pitdicker.github.io/Writing-a-seqlock-in-Rust/)
///   - [Can Seqlocks Get Along With Programming Language Memory Models?](https://www.hpl.hp.com/techreports/2012/HPL-2012-68.pdf)
///   - [Crate seqlock.](https://docs.rs/seqlock/0.1.2/seqlock/)
#[derive(Debug, Default)]
pub struct AtomicCorrelationPoint {
    /// Значение счётчика тиков отслеживаемых часов.
    count: AtomicI64,

    /// - Нечётное значение в [`AtomicCorrelationPoint::sequence`] означает,
    ///   что писатель начал обновлять структуру [`AtomicCorrelationPoint`], но ещё не закончил.
    ///   Если читатель обнаруживает структуру в таком состоянии,
    ///   он должен подождать пока писатель закончит обновление.
    /// - Чётное значение в [`AtomicCorrelationPoint::sequence`] означает,
    ///   что значение структуры [`AtomicCorrelationPoint`] согласованно.
    ///   И читатель может его использовать при дополнительном условии,
    ///   что чтение [`AtomicCorrelationPoint::sequence`] вернуло один и тот же результат
    ///   до чтения и после чтения остальных полей.
    sequence: AtomicU64,

    /// Значение счётчика тактов процессора.
    tsc: AtomicI64,
}

impl AtomicCorrelationPoint {
    /// Возвращает [`AtomicCorrelationPoint`], заполненную нулями.
    /// Аналогична [`AtomicCorrelationPoint::default()`], но доступна в константном контексте.
    pub const fn new() -> Self {
        Self {
            count: AtomicI64::new(0),
            sequence: AtomicU64::new(0),
            tsc: AtomicI64::new(0),
        }
    }

    /// Возвращает `true`, если `self` привязан к какому-нибудь такту процессора.
    pub fn is_valid(&self) -> bool {
        self.tsc.load(Ordering::Relaxed) != 0
    }

    /// Атомарно инкрементирует `count` и
    /// одновременно записывает заданное в `tsc` значение счётчика тактов процессора.
    pub fn inc(
        &self,
        tsc: i64,
    ) {
        self.sequence.fetch_add(1, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        self.tsc.store(tsc, Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// Атомарно записывает заданное `correlation_point` значение.
    pub fn store(
        &self,
        correlation_point: CorrelationPoint,
    ) {
        self.sequence.fetch_add(1, Ordering::Relaxed);
        
        self.count.store(correlation_point.count(), Ordering::Relaxed);
        self.tsc.store(correlation_point.tsc(), Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// Атомарно читает значение [`CorrelationPoint`] из структуры [`AtomicCorrelationPoint`].
    pub fn load(&self) -> CorrelationPoint {
        loop {
            let seq1 = self.sequence.load(Ordering::Acquire);
            while Self::is_locked(seq1) {
                hint::spin_loop();
                let seq = self.sequence.load(Ordering::Acquire);
                if !Self::is_locked(seq) {
                    break;
                }
            }
            let seq1 = self.sequence.load(Ordering::Acquire);
            if Self::is_locked(seq1) {
                continue;
            }
            
            let count = self.count.load(Ordering::Relaxed);
            let tsc = self.tsc.load(Ordering::Relaxed);
            
            let seq2 = self.sequence.load(Ordering::Acquire);
            
            if seq1 == seq2 {
                return CorrelationPoint { count, tsc };
            }
            
            hint::spin_loop();
        }
    }

    /// Читает значение [`CorrelationPoint`] из структуры [`AtomicCorrelationPoint`].
    /// Возвращает [`Some`], если удалось прочитать согласованное значение.
    fn try_load(&self) -> Option<CorrelationPoint> {
        let seq1 = self.sequence.load(Ordering::Acquire);
        if Self::is_locked(seq1) {
            return None;
        }
        let count = self.count.load(Ordering::Relaxed);
        let tsc = self.tsc.load(Ordering::Relaxed);
        let seq2 = self.sequence.load(Ordering::Acquire);
        if seq1 == seq2 {
            Some(CorrelationPoint { count, tsc })
        } else {
            None
        }
    }

    /// Возвращает `true`, если значение `sequence` означает,
    /// что захвачена блокировка на запись.
    fn is_locked(sequence: u64) -> bool {
        !sequence.is_multiple_of(2)
    }
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use super::CorrelationPoint;

    pub use super::AtomicCorrelationPoint;

    pub fn new_point(
        count: i64,
        tsc: i64,
    ) -> CorrelationPoint {
        CorrelationPoint::new(count, tsc)
    }

    pub fn try_load(point: &AtomicCorrelationPoint) -> Option<CorrelationPoint> {
        point.try_load()
    }
}
