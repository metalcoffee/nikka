use static_assertions::const_assert_eq;

use crate::system_info;

use super::Hz;

/// Базовая частота PIT в герцах, из документации.
const BASE: u32 = 2 * 2 * 5 * 59659;

/// Делитель базовой частоты для конфигурирования частоты тиков.
pub const DIVISOR: u32 = BASE / TICKS_PER_SECOND;

/// Сконфигурированная частота тиков PIT.
///
/// Выбрана так, чтобы делить базовую частоту PIT без остатка.
pub const TICKS_PER_SECOND: u32 = 2 * 2 * 5;

const_assert_eq!(TICKS_PER_SECOND * DIVISOR, BASE);

/// Устаревший
/// [программируемый таймер](https://en.wikipedia.org/wiki/Programmable_interval_timer)
/// [Intel 8253/8254](https://en.wikipedia.org/wiki/Intel_8253).
///
/// [Синглтон](https://en.wikipedia.org/wiki/Singleton_pattern),
/// позволяющий узнать показания
/// [Intel 8253/8254](https://en.wikipedia.org/wiki/Intel_8253)
/// ([programmable interval timer, PIT](https://en.wikipedia.org/wiki/Programmable_interval_timer))
/// как из ядра, так и
/// из [пространства пользователя](https://en.wikipedia.org/wiki/User_space_and_kernel_space).
pub struct Pit;

impl Pit {
    /// Оценка частоты процессора с точки зрения PIT.
    pub fn tsc_per_second() -> Option<Hz> {
        system_info().pit().tsc_per_second()
    }
}
