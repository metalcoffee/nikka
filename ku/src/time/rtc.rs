use chrono::{
    DateTime,
    Utc,
};

use crate::system_info;

use super::{
    Hz,
    Tsc,
    correlation_interval::CorrelationInterval,
};

/// Частота тиков RTC.
pub const TICKS_PER_SECOND: i64 = 1;

/// [Часы реального времени](https://en.wikipedia.org/wiki/Real-time_clock).
///
/// [Синглтон](https://en.wikipedia.org/wiki/Singleton_pattern),
/// позволяющий узнать показания часов реального времени как из ядра, так и
/// из [пространства пользователя](https://en.wikipedia.org/wiki/User_space_and_kernel_space).
pub struct Rtc;

impl Rtc {
    /// Переводит номер такта процессора `tsc` в дату и время по показаниям PIT.
    pub fn datetime<const PARTS_PER_SECOND: i64>(tsc: Tsc) -> DateTime<Utc> {
        let rtc = system_info().rtc();
        CorrelationInterval::datetime::<PARTS_PER_SECOND>(rtc, tsc)
    }

    /// Оценка частоты процессора с точки зрения RTC.
    pub fn tsc_per_second() -> Option<Hz> {
        let rtc = system_info().rtc();
        rtc.tsc_per_second()
    }
}
