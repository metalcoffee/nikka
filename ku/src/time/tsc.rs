use core::{
    arch::x86_64,
    fmt,
    iter,
    mem,
};

use chrono::Duration;
use itertools::Itertools;
use num_traits::{
    NumCast,
    cast,
};
// FloatCore is used for a substitute of `f64::abs()`.
// `f64::abs()` is declared in `std` whilst this is a `#[no_std]` library.
// Moreover, `f64::abs()` from `std` uses FPU but this library is used in the kernel
// which is compiled with `+soft-float`.
// However, rustc fails to recognize that FloatCore is really used.
#[allow(unused_imports)]
use num_traits::float::FloatCore;
use number_prefix::NumberPrefix;
use serde::{
    Deserialize,
    Serialize,
};

use crate::error::{
    Error,
    Error::{
        NoData,
        Overflow,
    },
    Result,
};

use super::{
    Hz,
    MSECS_PER_SEC,
    NSECS_PER_SEC,
    pit8254::Pit,
    rtc::Rtc,
};

/// Описывает момент времени, храня значение счётчика тактов процессора.
///
/// Похожа на стандартную, но недоступную нам в `#[no_std]`--окружении структуру
/// [`std::time::Instant`](https://doc.rust-lang.org/std/time/struct.Instant.html).
/// Обе описывают
/// [монотонно возрастающее время](https://blog.codeminer42.com/the-monotonic-clock-and-why-you-should-care-about-it/).
/// То есть время, не ходящее "назад" ни при переводе часов на летнее время,
/// ни при корректировке неточно идущих часов.
/// Но такое время может никак не соответствовать
/// [реальному времени](https://en.wikipedia.org/wiki/Civil_time).
///
/// # Note
///
/// [`Tsc`] задаёт корректно определенную и монотонно возрастающую точку во времени
/// только на текущем компьютере и только до его перезапуска.
/// Её бессмысленно сохранять в персистентном хранилище вроде файла или посылать по сети.
/// Типажи [`Serialize`] и [`Deserialize`] используются только для
/// передачи [`Tsc`] между ядром и пространством пользователя.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Tsc(i64);

/// Описывает интервал между двумя моментами времени [`Tsc`].
///
/// Наивно предполагает, что используемый ею счётчик тактов процессора как минимум
/// инвариантен и согласован между процессорами.
/// Похожа на стандартную, но недоступную нам в `#[no_std]`--окружении структуру
/// [`std::time::Duration`](https://doc.rust-lang.org/std/time/struct.Duration.html).
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct TscDuration(i64);

impl Tsc {
    /// Возвращает [`Tsc`] с заданным номером такта процессора.
    #[inline(always)]
    pub fn new(tsc: i64) -> Self {
        Self(tsc)
    }

    /// Возвращает [`Tsc`] с номером текущего такта процессора.
    #[inline(always)]
    pub fn now() -> Self {
        Self(tsc())
    }

    /// Возвращает [`TscDuration`] с количеством тактов процессора,
    /// которое прошло от `self` до текущего момента.
    #[inline(always)]
    pub fn elapsed(&self) -> TscDuration {
        TscDuration(tsc() - self.0)
    }

    /// Возвращает [`TscDuration`] с количеством тактов процессора,
    /// которое прошло от `self` до текущего момента.
    /// И записывает в `self` новый текущий номер такта процессора.
    #[inline(always)]
    pub fn lap(&mut self) -> TscDuration {
        let lap_end = tsc();
        TscDuration(lap_end - mem::replace(&mut self.0, lap_end))
    }

    /// Возвращает `true`, если с момента создания этого [`Tsc`]
    /// прошло не менее `duration` времени.
    ///
    /// # Note
    ///
    /// Если ещё не прошло два тика [`Rtc`] или [`Pit`],
    /// то [`Duration`] и [`TscDuration`] сравнить нельзя,
    /// см. [`TscDuration::try_from::<Duration>()`].
    /// В этом случае считается, что один такт процессора
    /// происходит за одну наносекунду.
    pub fn has_passed(
        &self,
        duration: Duration,
    ) -> bool {
        let elapsed = self.elapsed();
        elapsed.try_into().unwrap_or_else(|_| Duration::nanoseconds(elapsed.0)) >= duration
    }

    /// Возвращает номер такта процессора, записанный в [`Tsc`].
    pub(super) fn get(&self) -> i64 {
        self.0
    }
}

impl TscDuration {
    /// Создает [`TscDuration`] из количества тактов процессора.
    pub(super) fn new(tsc: i64) -> Self {
        Self(tsc)
    }

    /// Возвращает количество тактов процессора из [`TscDuration`] в виде [`f64`].
    pub fn into_f64(self) -> f64 {
        let tsc: u64 = self.0.try_into().expect("duration should not be negative");
        into_f64(tsc)
    }
}

impl fmt::Debug for TscDuration {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        match NumberPrefix::decimal(self.into_f64()) {
            NumberPrefix::Standalone(_) => {
                write!(formatter, "{} tsc", self.0)
            },
            NumberPrefix::Prefixed(prefix, value) => {
                write!(formatter, "{:.3} {}tsc", value, prefix.symbol())
            },
        }
    }
}

impl fmt::Display for TscDuration {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        if let Some(hz) = tsc_per_second() {
            let hz = into_f64(hz.get());
            let value = self.into_f64() / hz;

            match NumberPrefix::decimal(value) {
                NumberPrefix::Standalone(value) => {
                    let (value, prefix) = fractional_prefix(value);
                    write!(formatter, "{value:.3} {prefix}s")
                },
                NumberPrefix::Prefixed(prefix, value) => {
                    write!(formatter, "{:.3} {}s", value, prefix.symbol())
                },
            }
        } else {
            (self as &dyn fmt::Debug).fmt(formatter)
        }
    }
}

impl TryFrom<Duration> for TscDuration {
    type Error = Error;

    /// Преобразует [`Duration`] в [`TscDuration`]:
    ///   - С помощью [`Rtc`], если уже прошло два тика [`Rtc`].
    ///   - Иначе, с помощью [`Pit`], если уже прошло два тика [`Pit`].
    ///
    /// Возвращает ошибку [`Error::NoData`], если пока ни [`Rtc`] ни [`Pit`] не тикнули дважды.
    fn try_from(duration: Duration) -> Result<Self> {
        let hz = tsc_per_second().ok_or(NoData)?;
        let hz = into_f64(hz.get());

        let tsc = if let Some(nanoseconds) = duration.num_nanoseconds() {
            hz * into_f64(nanoseconds) / into_f64(NSECS_PER_SEC)
        } else {
            hz * into_f64(duration.num_milliseconds()) / into_f64(MSECS_PER_SEC)
        };

        Ok(TscDuration(tsc as i64))
    }
}

impl TryFrom<TscDuration> for Duration {
    type Error = Error;

    /// Преобразует [`TscDuration`] в [`Duration`]:
    ///   - С помощью [`Rtc`], если уже прошло два тика [`Rtc`].
    ///   - Иначе, с помощью [`Pit`], если уже прошло два тика [`Pit`].
    ///
    /// Возвращает ошибку [`Error::NoData`], если пока ни [`Rtc`] ни [`Pit`] не тикнули дважды.
    fn try_from(tsc_duration: TscDuration) -> Result<Self> {
        let hz = tsc_per_second().ok_or(NoData)?;
        let hz = i64::try_from(hz.get())?;

        let seconds = Self::seconds(tsc_duration.0 / hz);
        let nanoseconds = Self::nanoseconds((tsc_duration.0 % hz) * NSECS_PER_SEC / hz);

        seconds.checked_add(&nanoseconds).ok_or(Overflow)
    }
}

// ANCHOR: tsc
/// Возвращает
/// [номер текущего такта процессора](https://en.wikipedia.org/wiki/Time_Stamp_Counter)
/// в некоторый момент времени своего исполнения.
#[inline(always)]
pub fn tsc() -> i64 {
    if cfg!(miri) {
        return 1;
    }

    // Do not use `x86::fence::lfence()` and `x86::time::rdtsc()`
    // because they are not marked as `#[inline]`.
    unsafe {
        x86_64::_mm_lfence();
        x86_64::_rdtsc()
            .try_into()
            .expect("i64 overflow when storing TSC is expected only after tens of years of uptime")
    }
}
// ANCHOR_END: tsc

/// Возвращает частоту процессора, вычисленную:
///   - С помощью [`Rtc`], если уже прошло два тика [`Rtc`].
///   - Иначе, с помощью [`Pit`], если уже прошло два тика [`Pit`].
///
/// Возвращает [`None`], если пока ни [`Rtc`] ни [`Pit`] не тикнули дважды.
fn tsc_per_second() -> Option<Hz> {
    Rtc::tsc_per_second().or_else(Pit::tsc_per_second)
}

/// Переводит дробное значение `value` в более удобную для человека форму.
///
/// Возвращает отмасштабированное значение и суффикс масштаба:
///   - [`m` --- милли-](https://en.wikipedia.org/wiki/Milli-),
///   - [`u` --- микро-](https://en.wikipedia.org/wiki/Micro-) или
///   - [`n` --- нано-](https://en.wikipedia.org/wiki/Nano-).
fn fractional_prefix(value: f64) -> (f64, &'static str) {
    // Alas, `number_prefix` does not support fractional prefixes.
    iter::successors(Some(value), |x| Some(x * 1000.0))
        .zip(["", "m", "u", "n"])
        .find_or_last(|x| x.0.abs() >= 1.0)
        .unwrap()
}

/// Преобразует [`i64`] или [`u64`]
/// (точнее любое 64-битное число, реализующее типаж [`NumCast`])
/// в [`f64`].
fn into_f64<T: NumCast>(value: T) -> f64 {
    assert_eq!(mem::size_of_val(&value), mem::size_of::<u64>());
    cast::cast(value).expect("a cast from i64/u64 to f64 can loose precision but should not fail")
}

#[cfg(test)]
mod test {
    use super::fractional_prefix as fp;

    #[test]
    fn fractional_prefix() {
        assert!(check(1.0, 1.0, ""));
        assert!(check(100.0, 100.0, ""));
        assert!(check(1_000_000.0, 1_000_000.0, ""));

        assert!(check(0.1, 100.0, "m"));
        assert!(check(0.001, 1.0, "m"));

        assert!(check(0.000_1, 100.0, "u"));
        assert!(check(0.000_001, 1.0, "u"));

        assert!(check(0.000_000_1, 100.0, "n"));
        assert!(check(0.000_000_001, 1.0, "n"));

        assert!(check(0.000_000_000_1, 0.1, "n"));
        assert!(check(0.000_000_000_001, 0.001, "n"));

        fn check(
            argument: f64,
            result: f64,
            prefix: &str,
        ) -> bool {
            for sign in [1.0, -1.0] {
                if !equal(fp(sign * argument), (sign * result, prefix)) {
                    return false;
                }
            }
            true
        }

        fn equal(
            a: (f64, &str),
            b: (f64, &str),
        ) -> bool {
            (a.0 - b.0).abs() < 1e-6 && a.1 == b.1
        }
    }
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use super::Tsc;

    pub fn forge_tsc(tsc: i64) -> Tsc {
        Tsc(tsc)
    }
}
