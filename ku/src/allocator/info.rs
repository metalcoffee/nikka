use core::{
    cmp,
    fmt,
    hint,
    mem,
    ops::{
        AddAssign,
        Sub,
    },
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

use crate::{
    error::{
        Error::Overflow,
        Result,
    },
    memory::{
        Page,
        Size,
        Virt,
    },
};

/// Статистика аллокатора общего назначения.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Info {
    /// Сколько памяти в байтах было выделено аллокатором.
    allocated: Counter,

    /// Количество запросов к аллокатору.
    allocations: Counter,

    /// Количество виртуальных страниц, которые аллокатор выделил для удовлетворения запросов.
    pages: Counter,

    /// Сколько памяти в байтах было запрошено у аллокатора.
    requested: Counter,
}

impl Info {
    /// Инициализирует статистику аллокатора нулями.
    pub(super) const fn new() -> Self {
        Self {
            allocated: Counter::new(),
            allocations: Counter::new(),
            pages: Counter::new(),
            requested: Counter::new(),
        }
    }

    /// Сколько памяти в байтах было выделено аллокатором.
    pub fn allocated(&self) -> Counter {
        self.allocated
    }

    /// Количество запросов к аллокатору.
    pub fn allocations(&self) -> Counter {
        self.allocations
    }

    /// Количество виртуальных страниц, которые аллокатор выделил для удовлетворения запросов.
    pub fn pages(&self) -> Counter {
        self.pages
    }

    /// Сколько памяти в байтах было запрошено у аллокатора.
    pub fn requested(&self) -> Counter {
        self.requested
    }

    /// Сколько памяти в байтах потеряно на фрагментацию в текущий момент времени.
    /// Равно `allocated().balance() - requested().balance()`.
    ///
    /// Если эта [`Info`] --- разность между двумя другими, то
    /// [`Info::fragmentation_loss()`] формально может быть отрицательным.
    /// В этом случае возвращается `0`.
    pub fn fragmentation_loss(&self) -> usize {
        (self.pages.balance() * Page::SIZE).saturating_sub(self.requested.balance())
    }

    /// Сколько памяти в процентах потеряно на фрагментацию в текущий момент времени.
    /// Равно `100.0 * fragmentation_loss() / allocated().balance()`.
    pub fn fragmentation_loss_percentage(&self) -> f64 {
        let total = self.pages.balance() * Page::SIZE;

        self.fragmentation_loss() as f64 / cmp::max(total, 1) as f64 * 100.0
    }

    /// Проверка, соблюдены ли ожидаемые инварианты:
    ///   - Количество и объёмы освобождений памяти не должны превосходить
    ///     соответствующие величины для выделений памяти.
    ///   - Реально выделенный объём памяти не может быть меньше запрошенного.
    ///   - Текущий объём выделенной память не может быть больше суммарного объёма
    ///     текущих выделенных страниц.
    pub fn is_valid(&self) -> bool {
        self.allocated.is_valid() &&
            self.allocations.is_valid() &&
            self.pages.is_valid() &&
            self.requested.is_valid() &&
            self.requested.balance() <= self.allocated.balance() &&
            self.requested.negative() <= self.allocated.negative() &&
            self.allocated.balance() <= self.pages.balance() * Page::SIZE
    }

    /// Добавляет в счётчики одну операцию выделения памяти:
    ///   - Запрошено `requested` байт.
    ///   - Реально выделено `allocated` байт.
    ///   - Для операции дополнительно пришлось выделить `allocated_pages` виртуальных страниц.
    pub fn allocation(
        &mut self,
        requested: usize,
        allocated: usize,
        allocated_pages: usize,
    ) {
        self.allocated.increase(allocated);
        self.allocations.increase(1);
        self.pages.increase(allocated_pages);
        self.requested.increase(requested);
    }

    /// Добавляет в счётчики одну операцию освобождения памяти:
    ///   - Запрошено освобождение `requested` байт.
    ///   - Реально освобождено `deallocated` байт.
    ///   - При этом освободились `deallocated_pages` виртуальных страниц.
    pub fn deallocation(
        &mut self,
        requested: usize,
        deallocated: usize,
        deallocated_pages: usize,
    ) {
        self.allocated.decrease(deallocated);
        self.allocations.decrease(1);
        self.pages.decrease(deallocated_pages);
        self.requested.decrease(requested);
    }

    /// Учитывает в счётчиках выделение `allocated_pages` виртуальных страниц.
    pub fn pages_allocation(
        &mut self,
        allocated_pages: usize,
    ) {
        self.pages.increase(allocated_pages);
    }

    /// Учитывает в счётчиках освобождение `deallocated_pages` виртуальных страниц.
    pub fn pages_deallocation(
        &mut self,
        allocated_pages: usize,
    ) {
        self.pages.decrease(allocated_pages);
    }

    /// Поддерживается ли статистика аллокатора общего назначения.
    /// Равно `true`, если включена опция `allocator-statistics`.
    pub const IS_SUPPORTED: bool = true;
}

impl AddAssign for Info {
    fn add_assign(
        &mut self,
        other: Info,
    ) {
        self.allocated += other.allocated;
        self.allocations += other.allocations;
        self.pages += other.pages;
        self.requested += other.requested;
    }
}

impl Sub<Info> for Info {
    type Output = Result<Self>;

    fn sub(
        self,
        rhs: Info,
    ) -> Self::Output {
        Ok(Self {
            allocated: (self.allocated - rhs.allocated)?,
            allocations: (self.allocations - rhs.allocations)?,
            pages: (self.pages - rhs.pages)?,
            requested: (self.requested - rhs.requested)?,
        })
    }
}

/// Предназначена для конкурентного доступа к статистике аллокатора.
///
/// Реализует [неблокирующую синхронизацию](https://en.wikipedia.org/wiki/Non-blocking_algorithm)
/// для согласованного доступа к полям [`Info`].
/// Использует упрощённый [sequence lock](https://en.wikipedia.org/wiki/Seqlock).
///
/// См. также:
///   - [Writing a seqlock in Rust.](https://pitdicker.github.io/Writing-a-seqlock-in-Rust/)
///   - [Can Seqlocks Get Along With Programming Language Memory Models?](https://www.hpl.hp.com/techreports/2012/HPL-2012-68.pdf)
///   - [Crate seqlock.](https://docs.rs/seqlock/0.1.2/seqlock/)
#[derive(Debug, Default)]
pub struct AtomicInfo {
    /// Сколько памяти в байтах было выделено аллокатором.
    allocated: AtomicCounter,

    /// Количество запросов к аллокатору.
    allocations: AtomicCounter,

    /// Количество виртуальных страниц, которые аллокатор выделил для удовлетворения запросов.
    pages: AtomicCounter,

    /// Сколько памяти в байтах было запрошено у аллокатора.
    requested: AtomicCounter,

    /// - Нечётное значение в [`AtomicInfo::sequence`] означает,
    ///   что писатель начал обновлять структуру [`AtomicInfo`], но ещё не закончил.
    ///   Если читатель обнаруживает структуру в таком состоянии,
    ///   он должен подождать пока писатель закончит обновление.
    /// - Чётное значение в [`AtomicInfo::sequence`] означает,
    ///   что значение структуры [`AtomicInfo`] согласованно.
    ///   И читатель может его использовать при дополнительном условии,
    ///   что чтение [`AtomicInfo::sequence`] вернуло один и тот же результат
    ///   до чтения и после чтения остальных полей.
    sequence: AtomicUsize,
}

impl AtomicInfo {
    /// Возвращает [`AtomicInfo`], заполненную нулями.
    /// Аналогична [`AtomicInfo::default()`], но доступна в константном контексте.
    pub const fn new() -> Self {
        Self {
            allocated: AtomicCounter::new(),
            allocations: AtomicCounter::new(),
            pages: AtomicCounter::new(),
            requested: AtomicCounter::new(),
            sequence: AtomicUsize::new(0),
        }
    }

    /// Добавляет в счётчики одну операцию выделения памяти:
    ///   - Запрошено `requested` байт.
    ///   - Реально выделено `allocated` байт.
    ///   - Для операции дополнительно пришлось выделить `allocated_pages` виртуальных страниц.
    pub fn allocation(
        &self,
        requested: usize,
        allocated: usize,
        allocated_pages: usize,
    ) {
        self.sequence.fetch_add(1, Ordering::Acquire);
        self.allocated.increase(allocated);
        self.allocations.increase(1);
        if allocated_pages > 0 {
            self.pages.increase(allocated_pages);
        }
        self.requested.increase(requested);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// Добавляет в счётчики одну операцию освобождения памяти:
    ///   - Запрошено освобождение `requested` байт.
    ///   - Реально освобождено `deallocated` байт.
    ///   - При этом освободились `deallocated_pages` виртуальных страниц.
    pub fn deallocation(
        &self,
        requested: usize,
        deallocated: usize,
        deallocated_pages: usize,
    ) {
        self.sequence.fetch_add(1, Ordering::Acquire);
        self.allocated.decrease(deallocated);
        self.allocations.decrease(1);
        if deallocated_pages > 0 {
            self.pages.decrease(deallocated_pages);
        }
        self.requested.decrease(requested);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// Учитывает в счётчиках выделение `allocated_pages` виртуальных страниц.
    pub fn pages_allocation(
        &self,
        allocated_pages: usize,
    ) {
        self.sequence.fetch_add(1, Ordering::Acquire);
        self.pages.increase(allocated_pages);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// Учитывает в счётчиках освобождение `deallocated_pages` виртуальных страниц.
    pub fn pages_deallocation(
        &self,
        allocated_pages: usize,
    ) {
        self.sequence.fetch_add(1, Ordering::Acquire);
        self.pages.decrease(allocated_pages);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// Загрузить структуру [`Info`] из атомарного хранилища [`AtomicInfo`].
    pub fn load(&self) -> Info {
        let mut allocated;
        let mut allocations;
        let mut pages;
        let mut requested;

        loop {
            let mut sequence = self.sequence.load(Ordering::Acquire);

            while sequence % 2 == 1 {
                hint::spin_loop();
                sequence = self.sequence.load(Ordering::Acquire);
            }

            unsafe {
                allocated = self.allocated.load();
                allocations = self.allocations.load();
                pages = self.pages.load();
                requested = self.requested.load();
            }

            // Use the 'read-dont-modify-write' trick to be able to `Release`
            // the read section of the sequence lock.
            // See <https://www.hpl.hp.com/techreports/2012/HPL-2012-68.pdf>.
            let curr_sequence = self.sequence.fetch_add(0, Ordering::Release);

            if curr_sequence == mem::replace(&mut sequence, curr_sequence) {
                break;
            }
        }

        let info = Info {
            allocated,
            allocations,
            pages,
            requested,
        };

        assert!(info.is_valid());

        info
    }
}

impl fmt::Display for Info {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ allocations: {}, requested: {}, allocated: {}, pages: {}, loss: {} = {:.3}% }}",
            self.allocations,
            SizeCounter(self.requested),
            SizeCounter(self.allocated),
            self.pages,
            Size::new::<Virt>(self.fragmentation_loss()),
            self.fragmentation_loss_percentage(),
        )
    }
}

/// Счётчик для каждой из отслеживаемых величин,
/// разбитый на положительную часть для операций выделения памяти и
/// отрицательную часть для операций освобождения памяти.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counter {
    /// Суммарный размер соответствующего параметра для операций освобождения памяти.
    negative: usize,

    /// Суммарный размер соответствующего параметра для операций выделения памяти.
    positive: usize,
}

impl Counter {
    /// Инициализирует счётчик нулём.
    const fn new() -> Self {
        Self {
            negative: 0,
            positive: 0,
        }
    }

    /// Суммарный размер соответствующего параметра для операций освобождения памяти.
    pub fn negative(&self) -> usize {
        self.negative
    }

    /// Суммарный размер соответствующего параметра для операций выделения памяти.
    pub fn positive(&self) -> usize {
        self.positive
    }

    /// Баланс счётчика --- значение отслеживаемой величины в текущий момент.
    /// Равен разнице между положительной и отрицательной частью счётчика.
    pub fn balance(&self) -> usize {
        self.positive.checked_sub(self.negative).expect("negative balance detected")
    }

    /// Проверка, соблюдены ли ожидаемые инварианты.
    /// А именно, количество и объёмы выделений памяти не должны превосходить
    /// соответствующие величины для освобождений памяти.
    fn is_valid(&self) -> bool {
        self.negative <= self.positive
    }

    /// Добавить к счётчику величину `value`.
    fn increase(
        &mut self,
        value: usize,
    ) {
        self.positive += value;
    }

    /// Вычесть из счётчика величину `value`.
    fn decrease(
        &mut self,
        value: usize,
    ) {
        self.negative += value;
    }
}

impl fmt::Display for Counter {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{} - {} = ", self.positive(), self.negative())?;

        if self.is_valid() {
            write!(formatter, "{}", self.balance())
        } else {
            write!(formatter, "error")
        }
    }
}

impl AddAssign for Counter {
    fn add_assign(
        &mut self,
        other: Counter,
    ) {
        self.negative += other.negative;
        self.positive += other.positive;
    }
}

impl Sub<Counter> for Counter {
    type Output = Result<Self>;

    fn sub(
        self,
        rhs: Counter,
    ) -> Self::Output {
        let negative = self.negative.checked_sub(rhs.negative).ok_or(Overflow)?;
        let positive = self.positive.checked_sub(rhs.positive).ok_or(Overflow)?;

        Ok(Self { negative, positive })
    }
}

/// Атомарный счётчик для каждой из отслеживаемых величин,
/// разбитый на положительную часть для операций выделения памяти и
/// отрицательную часть для операций освобождения памяти.
#[derive(Debug, Default)]
struct AtomicCounter {
    /// Суммарный размер соответствующего параметра для операций освобождения памяти.
    negative: AtomicUsize,

    /// Суммарный размер соответствующего параметра для операций выделения памяти.
    positive: AtomicUsize,
}

impl AtomicCounter {
    /// Создаёт атомарный счётчик для каждой из отслеживаемых величин,
    /// имеющий нулевые положительную и отрицательную компоненты.
    /// Аналогична [`AtomicCounter::default()`], но доступна в константном контексте.
    const fn new() -> Self {
        Self {
            positive: AtomicUsize::new(0),
            negative: AtomicUsize::new(0),
        }
    }

    /// Загрузить значение из счётчика.
    ///
    /// # Safety
    ///
    /// Не гарантирует атомарность между положительной и отрицательной частями счётчика.
    /// Об этом должен позаботиться вызывающий код.
    unsafe fn load(&self) -> Counter {
        Counter {
            positive: self.positive(),
            negative: self.negative(),
        }
    }

    /// Суммарный размер соответствующего параметра для операций освобождения памяти.
    fn negative(&self) -> usize {
        self.negative.load(Ordering::Relaxed)
    }

    /// Суммарный размер соответствующего параметра для операций выделения памяти.
    fn positive(&self) -> usize {
        self.positive.load(Ordering::Relaxed)
    }

    /// Добавить к счётчику величину `value`.
    fn increase(
        &self,
        value: usize,
    ) {
        self.positive.fetch_add(value, Ordering::Relaxed);
    }

    /// Вычесть из счётчика величину `value`.
    fn decrease(
        &self,
        value: usize,
    ) {
        self.negative.fetch_add(value, Ordering::Relaxed);
    }
}

/// Вспомогательная структура для удобного форматирования счётчиков, отслеживающих байты.
struct SizeCounter(Counter);

impl fmt::Display for SizeCounter {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{} - {} = ",
            Size::new::<Virt>(self.0.positive()),
            Size::new::<Virt>(self.0.negative()),
        )?;

        if self.0.is_valid() {
            write!(formatter, "{}", Size::new::<Virt>(self.0.balance()))
        } else {
            write!(formatter, "error")
        }
    }
}
