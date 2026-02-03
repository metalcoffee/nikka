use core::{
    fmt,
    mem,
};

/// Требования к числовому идентификатору PCI устройств, производителей, классов и т.д.
pub trait IdTrait = Clone + Copy + Default + fmt::Debug + fmt::UpperHex;

/// Единый тип для идентификаторов PCI устройств, производителей, классов и т.д.
#[derive(Clone, Copy, Debug, Default)]
pub struct Id<T: IdTrait> {
    /// Числовое значение идентификатора.
    id: T,

    /// Имя, связанное с идентификатором, если оно есть в базе данных.
    name: Option<&'static str>,
}

impl<T: IdTrait> Id<T> {
    /// Создаёт идентификатор.
    pub(super) fn new(
        id: T,
        name: Option<&'static str>,
    ) -> Self {
        Self { id, name }
    }

    /// Возвращает числовое значение идентификатора.
    pub fn id(&self) -> T {
        self.id
    }

    /// Возвращает имя, связанное с идентификатором, если оно есть в базе данных.
    pub fn name(&self) -> Option<&'static str> {
        self.name
    }
}

impl<T: IdTrait> fmt::Display for Id<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        if let Some(name) = self.name() {
            write!(formatter, "{name}")
        } else {
            write!(
                formatter,
                "0x{:0width$X}",
                self.id(),
                width = 2 * mem::size_of::<T>()
            )
        }
    }
}
