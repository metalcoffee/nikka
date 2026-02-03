use core::fmt;

use bitflags::bitflags;

bitflags! {
    /// [Причина некорректности](https://wiki.osdev.org/Exceptions#Error_code)
    /// обращения к странице виртуальной памяти.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct PageFaultInfo: usize {
        /// Страница отображена, но нарушены флаги доступа к ней.
        const PRESENT = 1 << 0;

        /// Недопустимое обращение на запись.
        const WRITE = 1 << 1;

        /// Недопустимое обращение из пространства пользователя.
        const USER = 1 << 2;

        /// Таблица страниц содержит некорректные данные --- есть `1` в зарезервированном бите.
        const RESERVED_WRITE = 1 << 3;

        /// Недопустимое выполнение кода.
        const EXECUTE = 1 << 4;
    }
}

impl fmt::Display for PageFaultInfo {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{:#b} = {} | {} | {}",
            self.bits(),
            if self.contains(Self::RESERVED_WRITE) {
                "malformed page table (a reserved bit set)"
            } else if self.contains(Self::PRESENT) {
                "protection violation"
            } else {
                "non-present page"
            },
            if self.contains(Self::EXECUTE) {
                "execute"
            } else if self.contains(Self::WRITE) {
                "write"
            } else {
                "read"
            },
            if self.contains(Self::USER) {
                "user"
            } else {
                "kernel"
            },
        )
    }
}
