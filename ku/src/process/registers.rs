use core::{
    fmt,
    ops::Not,
};

use derive_more::{
    BitAnd,
    BitAndAssign,
    BitOr,
    BitOrAssign,
    BitXor,
    BitXorAssign,
};
use x86_64::registers::rflags;

use crate::{
    error::{
        Error::{
            self,
            InvalidArgument,
        },
        Result,
    },
    memory::size,
};

/// [Регистр флагов](https://en.wikipedia.org/wiki/FLAGS_register).
#[derive(
    Clone,
    Copy,
    BitAnd,
    BitAndAssign,
    BitOr,
    BitOrAssign,
    BitXor,
    BitXorAssign,
    Debug,
    Default,
    Eq,
    PartialEq,
)]
#[repr(transparent)]
pub struct RFlags(usize);

impl RFlags {
    /// Все допустимые флаги.
    pub const ALL: RFlags = RFlags(rflags::RFlags::all().bits() as usize);

    /// [Разрешает внешние прерывания](https://en.wikipedia.org/wiki/Interrupt_flag).
    pub const INTERRUPT_FLAG: RFlags = RFlags(rflags::RFlags::INTERRUPT_FLAG.bits() as usize);

    /// [Включает режим трассировки](https://en.wikipedia.org/wiki/Trap_flag).
    pub const TRAP_FLAG: RFlags = RFlags(rflags::RFlags::TRAP_FLAG.bits() as usize);

    /// Проверяет, что все указанные в `flags` флаги включены в `self`.
    pub fn contains(
        &self,
        flags: RFlags,
    ) -> bool {
        self.0 & flags.0 == flags.0
    }

    /// Возвращает [Input/Output Privilege level](https://en.wikipedia.org/wiki/Protection_ring#IOPL)
    pub fn iopl(&self) -> usize {
        (self.0 >> 12) & 0b11
    }

    /// Возвращает содержимое `self` в виде [`usize`].
    pub fn into_usize(&self) -> usize {
        self.0
    }

    /// Читает содержимое регистра флагов процессора.
    pub fn read() -> Self {
        Self(size::from(rflags::read_raw()))
    }

    /// Записывает `self` в регистр флагов процессора.
    ///
    /// # Safety
    ///
    /// Код должен быть готов к эффекту переключения флагов.
    /// В частности, до включения [`RFlags::INTERRUPT_FLAG`]
    /// должны быть настроены таблицы прерываний.
    pub unsafe fn write(&self) {
        unsafe {
            rflags::write_raw(size::into_u64(self.0));
        }
    }
}

impl fmt::Display for RFlags {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        /// Общепринятые сокращения для битов регистра флагов.
        static FLAGS: [&str; 22] = [
            "CF",
            "",
            "PF",
            "",
            "AF",
            "",
            "ZF",
            "SF",
            "TF",
            "IF",
            "DF",
            "OF",
            "",
            "",
            "NT",
            "",
            "RF",
            "VM",
            "AC",
            "VF",
            "VP",
            "ID",
        ];

        let mut separator = "";

        for i in (0 .. FLAGS.len()).rev() {
            if (self.0 >> i) & 1 != 0 && !FLAGS[i].is_empty() {
                write!(formatter, "{}{}", separator, FLAGS[i])?;
                separator = " ";
            }
        }

        if self.iopl() != 0 {
            write!(formatter, "{}IOPL-{}", separator, self.iopl())?;
        }

        Ok(())
    }
}

impl From<RFlags> for rflags::RFlags {
    fn from(rflags: RFlags) -> Self {
        Self::from_bits(size::into_u64(rflags.0)).expect("an incorrect flag is set in RFlags")
    }
}

impl From<RFlags> for usize {
    fn from(rflags: RFlags) -> Self {
        size::from(rflags::RFlags::from(rflags).bits())
    }
}

impl From<rflags::RFlags> for RFlags {
    fn from(rflags: rflags::RFlags) -> Self {
        Self(size::from(rflags.bits()))
    }
}

impl TryFrom<usize> for RFlags {
    type Error = Error;

    fn try_from(rflags: usize) -> Result<Self> {
        Ok(rflags::RFlags::from_bits(size::into_u64(rflags)).ok_or(InvalidArgument)?.into())
    }
}

impl Not for RFlags {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(Self::ALL.0 & !self.0)
    }
}
