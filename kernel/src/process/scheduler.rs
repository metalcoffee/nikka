use alloc::collections::VecDeque;

use lazy_static::lazy_static;
use x86_64::instructions;

use ku::sync::Spinlock;

use crate::{
    log::info,
    smp::LocalApic,
};

use super::{
    Pid,
    process::Process,
    table::Table,
};

/// Планировщик процессов.
/// Реализует простейшее
/// [циклическое исполнение процессов](https://en.wikipedia.org/wiki/Round-robin_scheduling).
pub struct Scheduler {
    /// Очередь готовых к исполнению процессов.
    queue: VecDeque<Pid>,
}

impl Scheduler {
    /// Инициализирует глобальный планировщик процессов с очередью на `count` процессов.
    pub fn init(count: usize) {
        *SCHEDULER.lock() = Scheduler {
            queue: VecDeque::with_capacity(count),
        }
    }

    /// Выполняет один цикл работы --- берёт первый процесс из очереди и
    /// исполняет его пользовательский код.
    /// Если в процессе выполнения пользовательского кода
    /// процесс был снят с CPU принудительно,
    /// перепланирует исполнение процесса, ставя его в конец очереди.
    /// Возвращает `true` если в очереди на исполнение нашёлся хотя бы один процесс.
    ///
    /// Должен корректно обрабатывать ситуацию, когда `pid` есть в очереди планирования,
    /// но соответствующего процесса уже нет в [`Table`].
    pub fn run_one() -> bool {
        let pid = match Self::dequeue() {
            Some(pid) => pid,
            None => return false,
        };

        if let Ok(process) = Table::get(pid) {
            let preempted = Process::enter_user_mode(process);

            if preempted {
                Self::enqueue(pid);
            }
        }

        true
    }

    /// В вечном цикле выполняет готовые процессы методом [`Scheduler::run_one()`].
    /// Если в очереди на исполнение процессов не нашлось,
    /// выключает процессор до прихода следующего прерывания,
    /// самое долгое --- до следующего тика таймера.
    pub(crate) fn run() -> ! {
        test_scaffolding::run_handler();

        let cpu = LocalApic::id();

        loop {
            if !Scheduler::run_one() {
                info!(cpu, "nothing to do");
                instructions::hlt();
            }
        }
    }

    /// Ставит процесс, заданный идентификатором `pid`, в очередь исполнения.
    pub fn enqueue(pid: Pid) {
        SCHEDULER.lock().queue.push_back(pid);
    }

    /// Достаёт из очереди первый готовый к исполнению процесс.
    fn dequeue() -> Option<Pid> {
        let pid = SCHEDULER.lock().queue.pop_front();
        info!("dequeue; pid = {pid:?}");
        pid
    }
}

lazy_static! {
    /// Планировщик процессов.
    /// Реализует простейшее
    /// [циклическое исполнение процессов](https://en.wikipedia.org/wiki/Round-robin_scheduling).
    static ref SCHEDULER: Spinlock<Scheduler> = Spinlock::new(Scheduler {
        queue: VecDeque::new(),
    });
}

#[doc(hidden)]
pub mod test_scaffolding {
    use core::{
        mem,
        sync::atomic::{
            AtomicBool,
            AtomicPtr,
            Ordering,
        },
    };

    use static_assertions::const_assert_eq;
    use x86_64::instructions;

    use super::{
        Pid,
        SCHEDULER,
    };

    pub fn scheduler_enable() {
        ENABLED.store(true, Ordering::Release);
    }

    pub fn scheduler_has_pid(pid: Pid) -> bool {
        SCHEDULER.lock().queue.contains(&pid)
    }

    pub fn set_handler(handler: fn()) {
        HANDLER.store(handler as *mut _, Ordering::Relaxed);
    }

    pub(super) fn run_handler() {
        let handler = HANDLER.load(Ordering::Relaxed);
        if !handler.is_null() {
            unsafe {
                const_assert_eq!(mem::size_of::<*const ()>(), mem::size_of::<fn()>());
                let handler = mem::transmute::<*const (), fn()>(handler);
                (handler)();
            }
        }
    }

    fn scheduler_wait() {
        loop {
            if ENABLED.load(Ordering::Acquire) {
                return;
            }

            instructions::hlt();
        }
    }

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static HANDLER: AtomicPtr<()> = AtomicPtr::new(scheduler_wait as *mut _);
}
