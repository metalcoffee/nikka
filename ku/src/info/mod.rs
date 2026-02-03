use core::{
    mem,
    ptr,
    sync::atomic::{
        AtomicPtr,
        Ordering,
    },
};

use static_assertions::const_assert_eq;

use super::{
    WriteBuffer,
    memory::{
        Block,
        Page,
        Virt,
    },
    process::Pid,
    time::{
        AtomicCorrelationInterval,
        pit8254,
        rtc,
    },
};

// Used in docs.
#[allow(unused)]
use crate as ku;

/// Информация о текущем процессе.
#[repr(C, align(4096))]
pub struct ProcessInfo {
    /// Буфер для асинхронного журналирования макросами библиотеки [`tracing`].
    log: WriteBuffer,

    /// Идентификатор текущего процесса.
    pid: Pid,

    /// Номер рекурсивной записи в таблице страниц.
    /// Позволяет процессу читать собственное отображение виртуальной памяти.
    recursive_mapping: usize,

    /// Область памяти, выделенная текущему процессу под стек режима пользователя.
    stack: Block<Virt>,

    /// Общая информации о системе.
    system_info: *const SystemInfo,
}

const_assert_eq!(mem::align_of::<ProcessInfo>(), Page::SIZE);
const_assert_eq!(mem::size_of::<ProcessInfo>() % Page::SIZE, 0);

impl ProcessInfo {
    /// Инициализирует [`ProcessInfo`].
    pub fn new(
        log: WriteBuffer,
        recursive_mapping: usize,
        system_info: *const SystemInfo,
    ) -> Self {
        Self {
            log,
            pid: Pid::Current,
            recursive_mapping,
            stack: Block::default(),
            system_info,
        }
    }

    /// Буфер для асинхронного журналирования макросами библиотеки [`tracing`].
    pub fn log(&mut self) -> &mut WriteBuffer {
        &mut self.log
    }

    /// Идентификатор текущего процесса.
    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// Устанавливает идентификатор текущего процесса.
    /// Используется только ядром.
    pub fn set_pid(
        &mut self,
        pid: Pid,
    ) {
        self.pid = pid;
    }

    /// Номер рекурсивной записи в таблице страниц.
    /// Позволяет процессу читать собственное отображение виртуальной памяти.
    pub fn recursive_mapping(&self) -> usize {
        self.recursive_mapping
    }

    /// Область памяти, выделенная текущему процессу под стек режима пользователя.
    pub fn stack(&self) -> Block<Virt> {
        self.stack
    }

    /// Устанавливает область памяти, выделенную текущему процессу под стек режима пользователя.
    pub fn set_stack(
        &mut self,
        stack: Block<Virt>,
    ) {
        self.stack = stack
    }

    /// Проверяет, принадлежит ли `address` структурам информации о процессе.
    ///
    /// Нужен, чтобы понимать, какую часть памяти не нужно копировать в другой процесс
    /// при той или иной разновидности создания процесса--клона.
    /// Для нового процесса ядро само сформирует аналогичные структуры в памяти.
    pub fn contains_address(
        &self,
        address: Virt,
    ) -> bool {
        [
            Block::<Virt>::from_ref(self),
            Block::<Virt>::from_ref(unsafe { &*self.system_info }),
            Block::<Virt>::from_ref(&self.log),
            self.log.block().into(),
        ]
        .iter()
        .any(|block| block.enclosing().contains_address(address))
    }
}

/// Общая информации о системе.
#[derive(Debug, Default)]
#[repr(C, align(4096))]
pub struct SystemInfo {
    /// Счётчик тиков PIT.
    pit: AtomicCorrelationInterval<{ pit8254::TICKS_PER_SECOND as i64 }>,

    /// Показания [часов реального времени](https://en.wikipedia.org/wiki/Real-time_clock)
    /// (Real-time clock, RTC).
    /// Позволяют в пространстве пользователя узнать текущее время
    /// с помощью функций модуля [`ku::time`].
    rtc: AtomicCorrelationInterval<{ rtc::TICKS_PER_SECOND }>,
}

const_assert_eq!(mem::align_of::<SystemInfo>(), Page::SIZE);
const_assert_eq!(mem::size_of::<SystemInfo>() % Page::SIZE, 0);

impl SystemInfo {
    /// Инициализирует [`SystemInfo`].
    pub const fn new() -> Self {
        Self {
            pit: AtomicCorrelationInterval::new(),
            rtc: AtomicCorrelationInterval::new(),
        }
    }

    /// Счётчик тиков PIT.
    pub fn pit(&self) -> &AtomicCorrelationInterval<{ pit8254::TICKS_PER_SECOND as i64 }> {
        &self.pit
    }

    /// Показания [часов реального времени](https://en.wikipedia.org/wiki/Real-time_clock)
    /// (Real-time clock, RTC).
    /// Позволяют в пространстве пользователя узнать текущее время
    /// с помощью функций модуля [`ku::time`].
    pub fn rtc(&self) -> &AtomicCorrelationInterval<{ rtc::TICKS_PER_SECOND }> {
        &self.rtc
    }
}

/// Информация о текущем процессе.
pub fn process_info() -> &'static mut ProcessInfo {
    let process_info = PROCESS_INFO.load(Ordering::Relaxed);
    unsafe { process_info.as_mut().expect("the process info is not initialized properly") }
}

/// Общая информации о системе.
pub fn system_info() -> &'static SystemInfo {
    let system_info = SYSTEM_INFO.load(Ordering::Relaxed);
    unsafe { system_info.as_ref().expect("the system info is not initialized properly") }
}

/// Устанавливает указатель на информации о текущем процессе.
///
/// Используется в пространстве пользователя на старте выполнения кода процесса.
/// Также устанавливает для текущего процесса указатель на общую информации о системе.
pub fn set_process_info(process_info: &'static mut ProcessInfo) {
    let system_info =
        unsafe { mem::transmute::<*const SystemInfo, *mut SystemInfo>(process_info.system_info) };
    PROCESS_INFO.store(process_info as *mut ProcessInfo, Ordering::Relaxed);
    SYSTEM_INFO.store(system_info, Ordering::Relaxed);
}

/// Устанавливает указатель на общую информации о системе.
///
/// Используется только в ядре,
/// чтобы сохранить указатель на одну общую на все процессы запись [`SystemInfo`].
pub fn set_system_info(system_info: &'static SystemInfo) {
    let system_info = system_info as *const SystemInfo;
    let system_info = unsafe { mem::transmute::<*const SystemInfo, *mut SystemInfo>(system_info) };
    SYSTEM_INFO.store(system_info, Ordering::Relaxed);
}

/// Указатель на информации о текущем процессе.
static PROCESS_INFO: AtomicPtr<ProcessInfo> = AtomicPtr::new(ptr::null_mut());

/// Указатель на общую информации о системе.
static SYSTEM_INFO: AtomicPtr<SystemInfo> = AtomicPtr::new(ptr::null_mut());
