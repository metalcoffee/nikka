/// Отладочная информация [`Callsite`] о точке вызова некоторой функции.
pub mod callsite;

#[cfg(not(miri))]
use core::arch::asm;
use core::{
    fmt,
    mem,
};

use derive_more::Display;

#[cfg(miri)]
use crate::error::Error::Unimplemented;
use crate::{
    error::Result,
    memory::{
        Block,
        Page,
        Virt,
        addr::{
            Tag,
            VirtTag,
        },
    },
    process::MiniContext,
};

pub use callsite::Callsite;

/// Поддержка печати трассировок стека.
///
/// Трассировку стека можно расшифровать командой `llvm-symbolizer`, например
/// ```console
/// ~/nikka$ cd kernel
/// ~/nikka/kernel$ cargo test --test 6-um-3-eager-fork
/// ...
/// 23:02:48.579 0 E panicked at 'I did my best, it wasn't much', user/eager_fork/src/main.rs:47:5; backtrace = 0x10008593 0x10008479 0x100085B8 0x10008CD9; pid = 2:1
/// ~/nikka/kernel$ echo '0x10008593 0x10008479 0x100085B8 0x10008CD9' | tr ' ' '\n' | llvm-symbolizer --exe ../target/kernel/user/eager_fork
/// eager_fork::fork_tree::h6af1cf920637510d
/// .../nikka/user/eager_fork/src/main.rs:47:5
///
/// eager_fork::main::h7eeb3104124da585
/// .../nikka/user/eager_fork/src/main.rs:40:1
///
/// main
/// .../nikka/user/lib/src/lib.rs:88:10
///
/// _start
/// .../nikka/user/lib/src/lib.rs:60:19
/// ```
///
/// Требует от компилятора
///   - генерации указателей фреймов (force-frame-pointers=yes) и
///   - расположения кода по фиксированным адресам (relocation-model=dynamic-no-pic).
///
/// Для этого нужно добавить в `.cargo/config.toml` опции
/// ```toml
/// [build]
///     rustflags = [
///         "--codegen", "force-frame-pointers=yes",
///         "--codegen", "relocation-model=dynamic-no-pic",
///     ]
/// ```
#[derive(Clone, Copy, Default)]
pub struct Backtrace {
    /// Адрес, ниже которого не может быть расположен внешний фрейм, --- стек растёт вниз.
    /// Снижает вероятность некорректного обращения к памяти
    /// при поиске конца списка стековых фреймов.
    lower_limit: Virt,

    /// Стек, знание которого позволяет найти конец списка стековых фреймов.
    /// Радикально снижает вероятность некорректного обращения к памяти
    /// при поиске конца списка стековых фреймов.
    /// Если не известен, устанавливается равным одной странице памяти,
    /// в которую указывает регистр `RBP`.
    stack: Block<Virt>,

    /// Текущий стековый фрейм.
    stack_frame: StackFrame,
}

impl Backtrace {
    /// Возвращает трассировку стека по значениям регистров `rbp` и `rsp`.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    /// В результате, функция вызвавшая [`Backtrace::current()`], в него не попадёт.
    #[inline(always)]
    pub fn new(
        rbp: usize,
        rsp: usize,
    ) -> Result<Self> {
        Self::new_impl(rbp, rsp, StackFrame::new(rbp)?)
    }

    /// Возвращает ошибку, так как для получения трассировки стека нужен ассемблер.
    /// А [Miri](https://github.com/rust-lang/miri) его не поддерживает.
    #[cfg(miri)]
    pub fn current() -> Result<Self> {
        Err(Unimplemented)
    }

    /// Возвращает трассировку текущего стека.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    /// В результате, функция вызвавшая [`Backtrace::current()`], в него не попадёт.
    #[cfg(not(miri))]
    #[inline(always)]
    pub fn current() -> Result<Self> {
        let rsp: usize;
        unsafe {
            asm!(
                "mov {0}, rsp",
                out(reg) rsp,
                options(nostack, nomem),
            );
        }

        Self::new(rbp(), rsp)
    }

    /// Возвращает трассировку стека по значениям регистров `rbp` и контексту `context`,
    /// который указывает на самый вложенный фрейм.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    /// В результате, функция вызвавшая [`Backtrace::current()`], в него не попадёт.
    #[inline(always)]
    pub fn with_context(
        rbp: usize,
        context: MiniContext,
    ) -> Result<Self> {
        let stack_frame = StackFrame {
            outer: rbp,
            return_address: context.rip().into_usize(),
        };

        Self::new_impl(rbp, context.rsp().into_usize(), stack_frame)
    }

    /// Возвращает трассировку текущего стека.
    ///
    /// `stack` --- текущий стек, знание которого позволяет найти конец списка стековых фреймов.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    /// В результате, функция вызвавшая [`Backtrace::with_stack()`], в него не попадёт.
    #[inline(always)]
    pub fn with_stack(stack: Block<Virt>) -> Result<Self> {
        let mut backtrace = Self::current()?;
        backtrace.stack = stack;
        Ok(backtrace)
    }

    /// Возвращает трассировку стека по значениям регистров `rbp` и `rsp`,
    /// и с самым вложенным фреймом `stack_frame`.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    /// В результате, функция вызвавшая [`Backtrace::current()`], в него не попадёт.
    #[inline(always)]
    fn new_impl(
        rbp: usize,
        rsp: usize,
        stack_frame: StackFrame,
    ) -> Result<Self> {
        let lower_limit = Virt::new(rsp)?;
        let stack_size = if cfg!(feature = "conservative-backtraces") {
            1
        } else {
            32 * Page::SIZE
        };
        let stack = Block::from_index(rbp, rbp + stack_size)?.enclosing().into();

        Ok(Self {
            lower_limit,
            stack,
            stack_frame,
        })
    }
}

impl Iterator for Backtrace {
    type Item = StackFrame;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stack_frame.outer == 0 {
            None
        } else {
            let next =
                self.stack_frame.outer(&mut self.lower_limit, self.stack).unwrap_or_default();

            Some(mem::replace(&mut self.stack_frame, next))
        }
    }
}

impl fmt::Debug for Backtrace {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "Backtrace:")?;

        for stack_frame in *self {
            write!(formatter, "\n  {stack_frame}")?;
        }

        Ok(())
    }
}

impl fmt::Display for Backtrace {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let mut separator = "";

        write!(formatter, "[")?;

        for stack_frame in *self {
            write!(formatter, "{separator}{stack_frame}")?;
            separator = " ";
        }

        write!(formatter, "]")
    }
}

/// Узел списка стековых фреймов.
#[derive(Clone, Copy, Debug, Default, Display)]
#[display("{:#X}", return_address)]
#[repr(C)]
pub struct StackFrame {
    /// Адрес внешнего стекового фрейма --- фрейма вызвавшей функции.
    outer: usize,

    /// Адрес возврата в вызвавшую функцию.
    return_address: usize,
}

impl StackFrame {
    /// Возвращает стековый фрейм по значению регистра `rbp`.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    #[inline(always)]
    pub fn new(rbp: usize) -> Result<Self> {
        Self::validate(rbp)
    }

    /// Возвращает ошибку, так как для получения текущего фрейма нужен ассемблер.
    /// А [Miri](https://github.com/rust-lang/miri) его не поддерживает.
    #[cfg(miri)]
    pub fn current() -> Result<Self> {
        Err(Unimplemented)
    }

    /// Возвращает текущий стековый фрейм.
    ///
    /// Мы указываем компилятору выполнить встраивание этой функции,
    /// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
    #[cfg(not(miri))]
    #[inline(always)]
    pub fn current() -> Result<Self> {
        Self::new(rbp())
    }

    /// Адрес возврата в вызвавшую функцию.
    pub fn return_address(&self) -> Virt {
        Virt::new(self.return_address).expect("incorrect stack frame")
    }

    /// Возвращает внешний стековый фрейма или `None`, если текущий фрейм самый внешний.
    fn outer(
        &self,
        lower_limit: &mut Virt,
        stack: Block<Virt>,
    ) -> Option<Self> {
        let outer_start = Virt::new(self.outer).expect("incorrect stack frame");

        if outer_start == Virt::default() ||
            !VirtTag::is_same_half(outer_start, *lower_limit) ||
            outer_start < *lower_limit
        {
            return None;
        }

        let outer_end = (outer_start + mem::size_of::<StackFrame>()).ok()?;
        let outer = Block::new(outer_start, outer_end).ok()?;

        if let Ok(new_lower_limit) = outer.start_address() + mem::size_of::<StackFrame>() {
            *lower_limit = new_lower_limit;
        }

        if stack.contains_block(outer) {
            Self::validate(self.outer).ok()
        } else {
            None
        }
    }

    /// Проверяет, что `stack_frame` является валидным адресом стекового фрейма и
    /// по нему расположен валидный стековый фрейм.
    /// В случае успеха возвращает копию этого стекового фрейма.
    ///
    /// # Note
    ///
    /// В случае успешного результата нет абсолютной гарантии,
    /// что по адресу `stack_frame` расположен именно стековый фрейм.
    /// Так как выполняемые проверки могут пройти успешно случайно,
    /// если по адресу `stack_frame` находится не стековый фрейм,
    /// а удовлетворяющий критериям проверок мусор.
    fn validate(stack_frame: usize) -> Result<Self> {
        let stack_frame = unsafe { *Virt::new(stack_frame)?.try_into_ref::<Self>()? };

        Virt::new(stack_frame.outer)?;
        Virt::new(stack_frame.return_address)?;

        Ok(stack_frame)
    }
}

/// Возвращает `true`, так как под
/// [Miri](https://github.com/rust-lang/miri)
/// не поддерживается получение значения регистра `RSP`.
#[cfg(miri)]
pub(crate) fn is_local_variable<T: ?Sized>(_ptr: *const T) -> bool {
    false
}

/// Возвращает `true`, если `ptr` похож на указатель на локальную переменную,
/// выделенную в пределах одной страницы от текущего указателя на вершину стека.
#[cfg(not(miri))]
pub(crate) fn is_local_variable<T: ?Sized>(ptr: *const T) -> bool {
    return is_top_stack_page_address(ptr as *const () as usize).unwrap_or(false);

    fn is_top_stack_page_address(virt: usize) -> Result<bool> {
        let current_stack_top = Virt::new(rsp())?;
        let top_stack_page = Block::from_element(Page::containing(current_stack_top))?;

        Ok(Block::<Virt>::from(top_stack_page).contains_index(virt))
    }
}

/// Возвращает содержимое регистра `RSP`.
#[cfg(not(miri))]
fn rsp() -> usize {
    let rsp: usize;
    unsafe {
        asm!(
            "mov {0}, rsp",
            out(reg) rsp,
        );
    }

    rsp
}

/// Возвращает содержимое регистра `RBP`.
///
/// Мы указываем компилятору выполнить встраивание этой функции,
/// чтобы не порождать дополнительный стековый фрейм под её вызов и не захламлять трассировку стека.
#[cfg(not(miri))]
#[inline(always)]
fn rbp() -> usize {
    let rbp: usize;
    unsafe {
        asm!(
            "mov {0}, rbp",
            out(reg) rbp,
            options(nostack, nomem),
        );
    }

    rbp
}

#[cfg(all(test, not(feature = "conservative-backtraces")))]
mod test {
    use core::hint;

    use sentinel_frame::with_sentinel_frame;

    use crate::memory::Virt;

    use super::Backtrace;

    #[derive(PartialEq, Eq, Debug, Default)]
    struct BacktraceStats {
        backtrace_depth: usize,
        found_sentinel: bool,
        stopped_by_sentinel: bool,
    }

    fn run_at_depth<T, Arg, F: FnOnce(Arg) -> T>(
        depth: usize,
        f: F,
        arg: Arg,
    ) -> T {
        // Prevent recursion unrolling when optimizations are enabled.
        let depth = hint::black_box(depth);

        if depth == 0 {
            f(arg)
        } else {
            run_at_depth(depth - 1, f, arg)
        }
    }

    fn capture_backtrace_fingerprint(backtrace_stats: &mut BacktraceStats) {
        if let Ok(mut bt) = Backtrace::current() {
            backtrace_stats.stopped_by_sentinel = true;

            for frame in bt.by_ref() {
                backtrace_stats.backtrace_depth += 1;

                if frame.return_address() == Virt::zero() {
                    backtrace_stats.stopped_by_sentinel = false;
                    break;
                }
            }

            backtrace_stats.found_sentinel = bt.stack_frame.return_address() == Virt::zero();
        }
    }

    #[with_sentinel_frame]
    fn write_backtrace_stats_to(
        depth: usize,
        backtrace_stats: &mut BacktraceStats,
    ) {
        run_at_depth(depth, capture_backtrace_fingerprint, backtrace_stats)
    }

    fn get_backtrace_stats(depth: usize) -> BacktraceStats {
        let mut stats = BacktraceStats::default();
        write_backtrace_stats_to(depth, &mut stats);
        stats
    }

    #[test]
    fn sentinel_frame() {
        for backtrace_depth in 0 .. 10 {
            let target_stats = get_backtrace_stats(backtrace_depth);

            assert!(target_stats.found_sentinel);
            assert!(target_stats.stopped_by_sentinel);
            assert!(target_stats.backtrace_depth >= backtrace_depth);
            assert!(target_stats.backtrace_depth < backtrace_depth + 7);

            for run_depth in 0 .. 5 {
                assert_eq!(
                    run_at_depth(run_depth, get_backtrace_stats, backtrace_depth),
                    target_stats,
                );
            }
        }
    }
}
