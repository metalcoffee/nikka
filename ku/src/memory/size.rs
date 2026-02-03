use core::{
    fmt,
    mem,
};

use number_prefix::NumberPrefix;
use static_assertions::const_assert_eq;

use crate::error::{
    Error,
    Result,
};

/// [Кибибайт](https://en.wikipedia.org/wiki/kibibyte)
#[allow(non_upper_case_globals)]
pub const KiB: usize = 1 << 10;

/// [Мебибайт](https://en.wikipedia.org/wiki/mebibyte)
#[allow(non_upper_case_globals)]
pub const MiB: usize = 1 << 20;

/// [Гибибайт](https://en.wikipedia.org/wiki/gibibyte)
#[allow(non_upper_case_globals)]
pub const GiB: usize = 1 << 30;

/// [Тебибайт](https://en.wikipedia.org/wiki/tebibyte)
#[allow(non_upper_case_globals)]
pub const TiB: usize = 1 << 40;

/// Преобразует в [`usize`] все примитивные типы, которые преобразуются без потерь в [`u64`].
pub fn from<T>(x: T) -> usize
where
    u64: From<T>,
{
    into_usize(u64::from(x))
}

/// Преобразует [`u64`] в [`usize`],
/// проверяя что эти типы имеют одинаковый размер на этапе компиляции.
pub const fn into_usize(x: u64) -> usize {
    const_assert_eq!(mem::size_of::<u64>(), mem::size_of::<usize>());
    x as usize
}

/// Преобразует [`usize`] в [`u64`],
/// проверяя что эти типы имеют одинаковый размер на этапе компиляции.
pub const fn into_u64(x: usize) -> u64 {
    const_assert_eq!(mem::size_of::<u64>(), mem::size_of::<usize>());
    x as u64
}

/// Преобразует [`usize`] в целый тип,
/// для которого есть [`TryFrom<u64>`], например в [`u32`].
/// Возвращает ошибку [`Error::Int`], если значение `x` не помещается в выбранный тип.
pub fn try_into<T: TryFrom<u64>>(x: usize) -> Result<T>
where
    Error: From<<T as TryFrom<u64>>::Error>,
{
    Ok(T::try_from(into_u64(x))?)
}

/// Типаж для указания размеров областей памяти определённого типа.
pub trait SizeOf {
    /// Размер области памяти.
    const SIZE_OF: usize;

    /// Возвращает количество областей, покрывающих `size` байт, округлённое вверх.
    fn count_up(size: usize) -> usize {
        size.div_ceil(Self::SIZE_OF)
    }
}

/// Обёртка для печати размеров областей памяти.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Size(usize);

impl Size {
    /// Возвращает размер для `count` областей памяти типа `T`.
    pub const fn new<T: SizeOf>(count: usize) -> Self {
        Self(count * T::SIZE_OF)
    }

    /// Возвращает размер для `count` областей памяти типа `T`.
    pub const fn new_u64<T: SizeOf>(count: u64) -> Self {
        Self::new::<T>(into_usize(count))
    }

    /// Возвращает размер для области памяти в `bytes` байт.
    pub const fn bytes(bytes: usize) -> Self {
        Self(bytes)
    }

    /// Возвращает размер для области памяти заданного в `slice` среза элементов типа `T`.
    pub const fn from_slice<T>(slice: &[T]) -> Self {
        Self(mem::size_of_val(slice))
    }

    /// Возвращает размер для области памяти элемента типа `T`.
    pub const fn of<T>() -> Self {
        Self(mem::size_of::<T>())
    }

    /// Возвращает размер в байтах.
    pub const fn num_bytes(&self) -> usize {
        self.0
    }
}

impl fmt::Display for Size {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        match NumberPrefix::binary(self.num_bytes() as f64) {
            NumberPrefix::Standalone(_) => {
                write!(formatter, "{} B", self.num_bytes())
            },
            NumberPrefix::Prefixed(prefix, value) => {
                write!(formatter, "{:.3} {}B", value, prefix.symbol())
            },
        }
    }
}
