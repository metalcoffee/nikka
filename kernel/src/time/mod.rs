/// Драйвер устаревшего таймера
/// [Intel 8253/8254](https://en.wikipedia.org/wiki/Intel_8253)
/// ([programmable interval timer, PIT](https://en.wikipedia.org/wiki/Programmable_interval_timer)).
/// Не представляет большого интереса, так как
/// [мы настроим](../../../lab/book/4-process-1-local-apic.html)
/// более современный
/// [таймер в APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller#APIC_timer).
pub(crate) mod pit8254;

/// Драйвер
/// [часов реального времени (Real-time clock, RTC)](https://en.wikipedia.org/wiki/Real-time_clock).
///
/// Они обычно условно независимы по питанию, так как снабжены
/// [батарейкой](https://en.wikipedia.org/wiki/Nonvolatile_BIOS_memory#CMOS_battery).
/// Отслеживают дату и время в реальном мире с точностью до секунды.
/// Соответствует
/// [спецификации микросхемы Motorola MC146818](https://pdf1.alldatasheet.com/datasheet-pdf/view/122156/MOTOROLA/MC146818.html).
pub mod rtc;

pub use ku::{
    Hz,
    Tsc,
    TscDuration,
    delay,
    now,
    now_ms,
    timer,
};

use crate::log::info;

/// Инициализирует
///   - таймер [Intel 8253/8254](https://en.wikipedia.org/wiki/Intel_8253) ([`pit8254`]) и
///   - [часы реального времени](https://en.wikipedia.org/wiki/Real-time_clock) ([`rtc`]).
pub(super) fn init() {
    pit8254::init();
    rtc::init();

    info!("time init");
}

#[doc(hidden)]
pub mod test_scaffolding {
    pub use super::rtc::test_scaffolding::{
        RegisterB,
        parse_hour,
    };
}
