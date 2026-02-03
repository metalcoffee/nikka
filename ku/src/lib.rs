#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

//! Общая для пространств ядра и пользователя библиотека.
//! ku --- **k**ernel && **u**ser.

#![allow(internal_features)]
#![deny(warnings)]
#![feature(adt_const_params)]
#![feature(allocator_api)]
#![feature(atomic_from_mut)]
#![feature(core_intrinsics)]
#![feature(gen_blocks)]
#![feature(int_roundings)]
#![feature(likely_unlikely)]
#![feature(maybe_uninit_fill, maybe_uninit_slice)]
#![feature(ptr_as_uninit)]
#![feature(slice_ptr_get)]
#![feature(step_trait)]
#![feature(trait_alias)]
#![no_std]
#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]

extern crate alloc;
extern crate rlibc;

/// Аллокаторы памяти общего назначения.
pub mod allocator;

/// Поддержка печати трассировок стека.
pub mod backtrace;

/// Коллекции элементов.
pub mod collections;

/// Перечисление для возможных ошибок [`Error`] и соответствующий [`Result`].
pub mod error;

/// Информации о системе, доступная пользовательским процессам.
///
/// Ядро предоставляет пользовательским процессам часть информации о системе,
/// сохраняя её в памяти, доступной пользователю на чтение.
/// Эта информация собирается в виде структуры общей информации о системе [`SystemInfo`]
/// и в виде структуры с информацией о текущем процессе [`ProcessInfo`].
pub mod info;

/// [Межпроцессное взаимодействие (Inter-process communication, IPC)](https://en.wikipedia.org/wiki/Inter-process_communication).
pub mod ipc;

/// Поддержка журналирования макросами библиотеки [`tracing`].
///
/// Сериализует сообщения в [`pipe`] и
/// передаёт их для журналирования в ядро через [`ProcessInfo::log()`].
/// Сериализация осуществляется с помощью [`serde`] в формате [`postcard`].
pub mod log;

/// Здесь собраны базовые примитивы для работы с памятью,
/// которые нужны и в ядре, и в пространстве пользователя.
pub mod memory;

/// Здесь собраны функции и структуры для работы с процессами,
/// которые нужны и в ядре, и в пространстве пользователя.
pub mod process;

/// Примитивы синхронизации [`Spinlock`] и [`SequenceLock`].
pub mod sync;

/// Здесь собраны базовые примитивы для работы со временем,
/// которые нужны и в ядре, и в пространстве пользователя.
pub mod time;

use x86_64::instructions::{
    self,
    interrupts,
};

pub use error::{
    Error,
    Result,
};
pub use info::{
    ProcessInfo,
    SystemInfo,
    process_info,
    set_process_info,
    set_system_info,
    system_info,
};
pub use ipc::pipe::{
    self,
    ReadBuffer,
    RingBufferReadTx,
    RingBufferWriteTx,
    WriteBuffer,
};
pub use sync::{
    sequence_lock::SequenceLock,
    spinlock::Spinlock,
};
pub use time::{
    Hz,
    Tsc,
    TscDuration,
    delay,
    now,
    now_ms,
    timer,
    tsc,
};

/// Останавливает процессор инструкцией
/// [`hlt`](https://www.felixcloutier.com/x86/hlt)
/// при запрещённых внешних прерываниях.
///
/// # Safety
///
/// Требует привилегированного режима работы.
#[cold]
#[inline(never)]
pub unsafe fn halt() -> ! {
    loop {
        interrupts::without_interrupts(instructions::hlt)
    }
}
