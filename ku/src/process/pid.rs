use core::{
    fmt,
    mem,
};

use static_assertions::const_assert;

use crate::error::{
    Error::InvalidArgument,
    Result,
};

// ANCHOR: pid
/// Идентификатор процесса.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum Pid {
    /// Текущий процесс, это удобно для использования тех системных вызовов,
    /// что принимают на вход [`Pid`].
    Current,

    /// Конкретный процесс, идентификатор которого состоит из
    /// номера слота в таблице процессов и эпохи этого слота.
    Id {
        /// Эпоха слота в таблице процессов.
        /// Позволяет сделать идентификаторы процессов уникальными
        /// на протяжении всего времени работы системы.
        epoch: u32,

        /// Номер слота в таблице процессов.
        /// Позволяет быстро находить процесс по его идентификатору в таблице процессов.
        slot: u16,
    },
}
// ANCHOR_END: pid

impl Pid {
    /// Максимальное поддерживаемое [`Pid`] количество одновременно работающих процессов.
    pub const MAX_COUNT: usize = (1 << Self::EPOCH_SHIFT);

    /// Сдвиг для значения [`Pid::Id::epoch`] при сериализации и десериализации [`Pid`] в [`usize`].
    const EPOCH_SHIFT: u32 = u16::BITS;

    /// Сдвиг для значения [`Pid::Id::slot`] при сериализации и десериализации [`Pid`] в [`usize`].
    const SLOT_MASK: usize = Self::MAX_COUNT - 1;

    /// Создаёт [`Pid`] с начальным значением [`Pid::Id::epoch`] для заданного `slot`.
    pub fn new(slot: usize) -> Self {
        Self::Id {
            epoch: 0,
            slot: slot.try_into().unwrap(),
        }
    }

    /// Позволяет десериализовать [`Pid`] из регистра при передаче в системные вызовы.
    pub fn from_usize(pid: usize) -> Result<Self> {
        if pid == usize::MAX - 1 {
            Ok(Self::Current)
        } else {
            let result = Self::Id {
                epoch: (pid >> Self::EPOCH_SHIFT) as u32,
                slot: (pid & Self::SLOT_MASK) as u16,
            };

            if result.into_usize() == pid {
                Ok(result)
            } else {
                Err(InvalidArgument)
            }
        }
    }

    /// Позволяет сериализовать [`Pid`] в регистр при передаче в системные вызовы.
    pub fn into_usize(&self) -> usize {
        const_assert!(mem::size_of::<Pid>() <= mem::size_of::<usize>());
        const_assert!(mem::size_of::<Option<Pid>>() <= mem::size_of::<usize>());

        match self {
            Self::Current => usize::MAX - 1,
            Self::Id { epoch, slot } => (*epoch as usize) << Self::EPOCH_SHIFT | (*slot as usize),
        }
    }

    /// Номер слота в таблице процессов.
    /// Позволяет быстро находить процесс по его идентификатору в таблице процессов.
    pub fn slot(&self) -> usize {
        let pid = match self {
            Self::Current => {
                unimplemented!();
            },
            pid => *pid,
        };

        if let Self::Id { slot, .. } = pid {
            slot.into()
        } else {
            panic!(
                "wrong pid {:?} encountered when processing pid {:?}",
                pid, self
            )
        }
    }

    /// Инкрементирует номер эпохи [`Pid::Id::epoch`] слота в таблице процессов.
    pub fn next_epoch(&mut self) {
        if let Self::Id { epoch, .. } = self {
            *epoch = epoch.checked_add(1).expect("epoch overflow");
        } else {
            panic!(
                "can not increment epoch in Pid::Current when processing pid {:?}",
                self,
            );
        }
    }
}

impl fmt::Debug for Pid {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        (self as &dyn fmt::Display).fmt(formatter)
    }
}

impl fmt::Display for Pid {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        match self {
            Self::Current => {
                write!(formatter, "<current>")
            },
            Self::Id { epoch, slot } => {
                write!(formatter, "{slot}:{epoch}")
            },
        }
    }
}
