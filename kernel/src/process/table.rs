use alloc::vec::Vec;
use core::fmt;

use lazy_static::lazy_static;

use ku::sync::spinlock::{
    Spinlock,
    SpinlockGuard,
};

use crate::{
    error::{
        Error::{
            InvalidArgument,
            NoProcess,
            NoProcessSlot,
        },
        Result,
    },
    log::info,
    time,
};

use super::{
    Pid,
    Process,
};

// Used in docs.
#[allow(unused)]
use crate::error::Error;

/// Слот таблицы процессов.
#[allow(clippy::large_enum_variant)]
enum Slot {
    /// Слот свободен.
    Free {
        /// Хранит эпоху последнего процесса, занимавшего этот слот.
        pid: Pid,

        /// Провязывает свободные слоты в интрузивный список.
        next: Option<Pid>,
    },

    /// Слот занят.
    Used {
        /// Процесс, находящийся в этом слоте таблицы процессов.
        process: Spinlock<Process>,
    },
}

impl fmt::Display for Slot {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        match self {
            Slot::Free { pid, next } => write!(formatter, "Free {{ pid: {pid}, next: {next:?} }}"),
            Slot::Used { process } => write!(formatter, "Process {}", *process.lock()),
        }
    }
}

/// Таблица процессов.
#[derive(Default)]
pub struct Table {
    /// Голова списка свободных слотов таблицы.
    free: Option<Pid>,

    /// Количество процессов в таблице.
    process_count: usize,

    /// Слоты таблицы процессов.
    table: Vec<Slot>,
}

impl Table {
    /// Он создаёт таблицу процессов [`Table`] размера `len` элементов, заполняя её
    /// пустыми слотами [`Slot::Free`] с соответствующими индексам слотов полями [`Pid::Id::slot`].
    /// Эти пустые слоты провязывает в односвязный список с головой в поле [`Table::free`].
    pub(super) fn new(len: usize) -> Self {
        let mut table = Vec::with_capacity(len);
        
        for slot in 0..len {
            let next = if slot + 1 < len {
                Some(Pid::new(slot + 1))
            } else {
                None
            };
            
            table.push(Slot::Free {
                pid: Pid::new(slot),
                next,
            });
        }

        Self {
            free: if len > 0 { Some(Pid::new(0)) } else { None },
            process_count: 0,
            table,
        }
    }

    /// Выделяет процессу `process` свободный слот таблицы и возвращает соответствующий [`Pid`].
    /// Если свободного слота нет, возвращает ошибку [`Error::NoProcessSlot`].
    pub(super) fn allocate(mut process: Process) -> Result<Pid> {
        let mut table = TABLE.lock();

        let pid = table.free.ok_or(NoProcessSlot)?;
        let slot = pid.slot();

        let next_free = if let Slot::Free { next, .. } = &table.table[slot] {
            *next
        } else {
            return Err(NoProcessSlot);
        };

        process.set_pid(pid);

        table.table[slot] = Slot::Used {
            process: Spinlock::new(process),
        };

        table.free = next_free;

        table.process_count += 1;

        info!("allocate; slot = {}; process_count = {}", table.table[slot], table.process_count);

        Ok(pid)
    }

    /// Удаляет процесс с заданным `pid`.
    /// При этом:
    ///   - Инкрементирует эпоху в освободившемся слоте.
    ///   - Вставляет слот в голову списка свободных слотов [`Table::free`].
    pub fn free(mut pid: Pid) -> Result<()> {
        let mut table = TABLE.lock();
        let slot = pid.slot();

        if slot >= table.table.len() {
            return Err(NoProcess);
        }

        let process = match &table.table[slot] {
            Slot::Used { process } => {
                let locked_process = process.lock();
                if locked_process.pid() != pid {
                    return Err(NoProcess);
                }
                drop(locked_process);
                process
            }
            Slot::Free { .. } => return Err(NoProcess),
        };

        info!("free; slot = {}; process_count = {}", process.lock(), table.process_count - 1);

        pid.next_epoch();

        table.table[slot] = Slot::Free {
            pid,
            next: table.free,
        };

        table.free = Some(pid);

        table.process_count -= 1;

        Ok(())
    }

    /// Возвращает захваченную спин-блокировку [`SpinlockGuard`] со структурой [`Process`]
    /// соответствующей идентификатору `pid`.
    /// Если процесса по указанному `pid` нет или тот же слот занят уже другим процессом,
    /// возвращает ошибку [`Error::NoProcess`].
    pub fn get(pid: Pid) -> Result<SpinlockGuard<'static, Process>> {
        let table = TABLE.lock();
        let slot = pid.slot();

        if slot >= table.table.len() {
            return Err(NoProcess);
        }

        match &table.table[slot] {
            Slot::Used { process } => {
                let process_guard = unsafe { forge_static_lifetime(process) }.lock();
                if process_guard.pid() != pid {
                    return Err(NoProcess);
                }
                Ok(process_guard)
            }
            Slot::Free { .. } => Err(NoProcess),
        }
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        assert!(
            self.table.is_empty(),
            "should not drop non-empty process table",
        );
    }
}

/// Обещает Rust, что слоты таблицы процессов [`Table`] имеют время жизни `'static`,
/// так как Rust не может проверить это самостоятельно.
///
/// Так как размер таблицы процессов [`Table`] после инициализации мы никогда не меняем,
/// и в частности не уменьшаем, время жизни каждого её слота --- практически `'static`.
unsafe fn forge_static_lifetime<T>(x: &T) -> &'static T {
    unsafe { &*(x as *const T) as &'static T }
}

lazy_static! {
    /// Таблица процессов.
    pub(super) static ref TABLE: Spinlock<Table> = Spinlock::new(Table::default());
}

#[doc(hidden)]
pub mod test_scaffolding {
    use crate::error::Result;

    use super::{
        Pid,
        Process,
        TABLE,
        Table,
    };

    pub fn init() {
        if TABLE.lock().table.is_empty() {
            *TABLE.lock() = Table::new(1);
        }
    }

    pub fn allocate(process: Process) -> Result<Pid> {
        Table::allocate(process)
    }
}
