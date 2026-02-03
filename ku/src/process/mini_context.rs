use core::{
    fmt,
    mem,
};

use memoffset::offset_of;

use crate::{
    error::Result,
    memory::{
        Block,
        Virt,
    },
};

/// Минимальная информация о контексте исполнения.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct MiniContext {
    /// Адрес кода --- содержимое регистра `rip`.
    rip: Virt,

    /// Адрес стека --- содержимое регистра `rsp`.
    rsp: Virt,
}

impl MiniContext {
    /// Создаёт [`MiniContext`] по значениям регистров `rip` и `rsp`.
    pub fn new(
        rip: Virt,
        rsp: Virt,
    ) -> Self {
        Self { rip, rsp }
    }

    /// Адрес кода --- содержимое регистра `rip`.
    pub fn rip(&self) -> Virt {
        self.rip
    }

    /// Адрес стека --- содержимое регистра `rsp`.
    pub fn rsp(&self) -> Virt {
        self.rsp
    }

    /// Выделяет на стеке [`MiniContext`] блок памяти под объект типа `T`.
    /// Не проверяет ни выравнивание, ни допустимость обращения к этому блоку памяти.
    pub fn push<T>(&mut self) -> Result<Block<Virt>> {
        let old_rsp = self.rsp;
        self.rsp = (old_rsp - mem::size_of::<T>())?;

        Block::new(self.rsp, old_rsp)
    }
}

impl fmt::Display for MiniContext {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{{ rip: {}, rsp: {} }}", self.rip, self.rsp)
    }
}

/// Смещение поля для регистра `rsp` контекста в структуре [`MiniContext`].
/// Позволяет обращаться к этому полю из ассемблерных вставок.
pub(super) const RSP_OFFSET_IN_MINI_CONTEXT: usize = offset_of!(MiniContext, rsp);
