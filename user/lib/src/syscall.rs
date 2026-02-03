use core::{
    arch::{
        asm,
        naked_asm,
    },
    mem,
    ptr,
    sync::atomic::{
        AtomicPtr,
        Ordering,
    },
};

use static_assertions::const_assert_eq;
use tracing_core::Level;

use ku::{
    error::{
        Error::InvalidArgument,
        Result,
    },
    log,
    memory::{
        Block,
        Page,
        USER_R,
        Virt,
        mmu::PageTableFlags,
        size,
    },
    process::{
        Pid,
        RSP_OFFSET_IN_TRAP_INFO,
        ResultCode,
        State,
        Syscall,
        TrapInfo,
    },
};

// Used in docs.
#[allow(unused)]
use crate::syscall;

/// Системный вызов [`syscall::exit()`].
///
/// Освобождает слот таблицы процессов и возвращается в контекст ядра,
/// из которого пользовательский процесс был запущен.
pub fn exit(code: usize) -> ! {
    syscall(Syscall::Exit, code, 0, 0, 0, 0).unwrap();

    unreachable!();
}

/// Системный вызов [`syscall::log_value()`].
///
/// Записывает в журнал строку `message` и число `value`.
pub fn log_value(
    level: Level,
    message: &str,
    value: usize,
) -> Result<()> {
    let block = Block::<Virt>::from_slice(message.as_bytes());

    syscall(
        Syscall::LogValue,
        size::from(u32::from(log::level_into_symbol(&level))),
        block.start_address().into_usize(),
        block.size(),
        value,
        0,
    )
    .map(|_| ())
}

/// Системный вызов [`syscall::sched_yield()`].
///
/// Перепланирует процесс в конец очереди готовых к исполнению процессов и
/// забирает у него CPU.
#[allow(unused_must_use)]
pub fn sched_yield() {
    syscall(Syscall::SchedYield, 0, 0, 0, 0, 0);
}

/// Системный вызов [`syscall::exofork()`].
///
/// Создаёт копию вызывающего процесса и возвращает исходному процессу [`Pid`] копии.
/// Внутри копии возвращает [`Pid::Current`].
/// При этом новый процесс создаётся практически без адресного пространства и не готовый к работе.
/// Поэтому он, в частности, не ставится в очередь планировщика.
/// Текущий контекст исходного процесса записывает в копию, чтобы в копии
/// вернуться туда же, куда происходит возврат из системного вызова для вызывающего процесса.
// Inline is needed for the correctness of exofork().
#[inline(always)]
pub fn exofork() -> Result<Pid> {
    let child_pid = syscall(Syscall::Exofork, 0, 0, 0, 0, 0)?;

    Pid::from_usize(child_pid)
}

/// Системный вызов [`syscall::map()`].
///
/// Отображает в памяти процесса, заданного `dst_pid`, блок страниц `dst_block`
/// с флагами доступа `flags`.
/// Если `dst_block.start_address()` равен нулю,
/// сам выбирает свободный участок адресного пространства размера `dst_block.size()`.
pub fn map(
    dst_pid: Pid,
    dst_block: Block<Page>,
    flags: PageTableFlags,
) -> Result<Block<Page>> {
    let address = syscall(
        Syscall::Map,
        dst_pid.into_usize(),
        dst_block.start_address().into_usize(),
        dst_block.size(),
        flags.bits(),
        0,
    )?;

    let start = Virt::new(address)?;
    let end = (start + dst_block.size())?;

    Block::new(Page::new(start)?, Page::new(end)?)
}

/// Системный вызов [`syscall::unmap()`].
///
/// Удаляет из виртуальной памяти целевого процесса `dst_pid` блок страниц `dst_block`.
pub fn unmap(
    dst_pid: Pid,
    dst_block: Block<Page>,
) -> Result<()> {
    syscall(
        Syscall::Unmap,
        dst_pid.into_usize(),
        dst_block.start_address().into_usize(),
        dst_block.size(),
        0,
        0,
    )
    .map(|_| ())
}

/// Системный вызов [`syscall::copy_mapping()`].
///
/// Создаёт копию отображения виртуальной памяти из вызывающего процесса
/// в процесс, заданный `dst_pid`.
/// Исходный диапазон задаёт `src_block`, целевой --- `dst_block`.
///
/// В целевом процессе диапазон должен быть отображён с флагами:
///   - `flags`, если `flags` --- [`Some`].
///   - постранично такими же, как в исходном диапазоне, если `flags` --- [`None`].
///
/// Не допускает целевое отображение с более широким набором флагов, чем исходное.
///
/// После выполнения у процессов появляется область
/// [разделяемой памяти](https://en.wikipedia.org/wiki/Shared_memory).
pub fn copy_mapping(
    dst_pid: Pid,
    src_block: Block<Page>,
    dst_block: Block<Page>,
    flags: Option<PageTableFlags>,
) -> Result<()> {
    if src_block.count() == dst_block.count() && flags.unwrap_or(USER_R).is_user() {
        syscall(
            Syscall::CopyMapping,
            dst_pid.into_usize(),
            src_block.start_address().into_usize(),
            dst_block.start_address().into_usize(),
            dst_block.size(),
            flags.map(|x| x.bits()).unwrap_or(0),
        )
        .map(|_| ())
    } else {
        Err(InvalidArgument)
    }
}

/// Системный вызов [`syscall::set_state()`].
///
/// Переводит целевой процесс, заданный идентификатором `dst_pid`, в заданное состояние `state`.
/// Ставит его в очередь планировщика в случае [`State::Runnable`].
pub fn set_state(
    dst_pid: Pid,
    state: State,
) -> Result<()> {
    syscall(
        Syscall::SetState,
        dst_pid.into_usize(),
        state.into(),
        0,
        0,
        0,
    )
    .map(|_| ())
}

// ANCHOR: set_trap_handler
/// Системный вызов [`syscall::set_trap_handler()`].
///
/// Устанавливает для целевого процесса, заданного идентификатором `dst_pid`,
/// пользовательский обработчик прерывания `trap_handler()` со стеком `trap_stack`.
pub fn set_trap_handler(
    dst_pid: Pid,
    trap_handler: fn(&TrapInfo),
    trap_stack: Block<Page>,
) -> Result<()> {
    // ANCHOR_END: set_trap_handler
    TRAP_HANDLER.store(trap_handler as *mut _, Ordering::Relaxed);

    syscall(
        Syscall::SetTrapHandler,
        dst_pid.into_usize(),
        trap_trampoline as *const () as usize,
        trap_stack.start_address().into_usize(),
        trap_stack.size(),
        0,
    )
    .map(|_| ())
}

// ANCHOR: syscall
/// Системный вызовов номер `number` с аргументами `arg0`--`arg4`.
// Inline is needed for the correctness of exofork().
#[inline(always)]
pub fn syscall(
    number: Syscall,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> Result<usize> {
    // ANCHOR_END: syscall
    let result_code: usize;
    let value: usize;
    
    unsafe {
        asm!(
            "push rbx",
            "push rbp",
        
            "mov r10, rcx",
            
            "syscall",
            
            "pop rbp",
            "pop rbx",
            
            inout("rax") usize::from(number) => result_code,
            inlateout("rdi") arg0 => value,
            in("rsi") arg1,
            in("rdx") arg2,
            in("rcx") arg3,
            in("r8") arg4,
            
            lateout("rcx") _,
            lateout("r11") _,
            lateout("rdx") _,
            lateout("rsi") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r12") _,
            lateout("r13") _,
            lateout("r14") _,
            lateout("r15") _,
        );
    }
    
    let result_code = ResultCode::try_from(result_code)
        .map_err(|_| ku::error::Error::InvalidArgument)?;
    
    let result: Result<()> = result_code.into();
    result.map(|_| value)
}

/// Получает управление, если в коде пользователя возникло исключение.
/// Сохраняет контекст исключения и передаёт управление обработчику `trap_handler()`
/// установленному с помощью [`syscall::set_trap_handler()`]
/// через вспомогательную функцию [`trap_handler_invoker()`].
/// После выполнения установленного обработчика `trap_handler()` восстанавливает
/// сохранённый контекст.
#[cold]
#[unsafe(naked)]
extern "C" fn trap_trampoline() -> ! {
    naked_asm!(
        "
            // TODO: your code here.

            call {trap_handler_invoker}

            // TODO: your code here.

            // Return to the point which caused this trap.
            // This relies on that trap_handler_invoker() has called TrapInfo::prepare_for_ret()
            // to put the trap site RIP on the trap site stack.
            ret
        ",

        // TODO: your code here.
        trap_handler_invoker = sym trap_handler_invoker,
    );
}

// ANCHOR: trap_handler_invoker
/// Передаёт управление обработчику `trap_handler()`
/// установленному с помощью [`syscall::set_trap_handler()`].
#[cold]
#[inline(never)]
extern "C" fn trap_handler_invoker(
    // rdi
    info: &mut TrapInfo,
) {
    // ANCHOR_END: trap_handler_invoker
    let trap_handler = TRAP_HANDLER.load(Ordering::Relaxed);
    if !trap_handler.is_null() {
        unsafe {
            const_assert_eq!(mem::size_of::<*const ()>(), mem::size_of::<fn(&TrapInfo)>());
            let trap_handler = mem::transmute::<*const (), fn(&TrapInfo)>(trap_handler);
            (trap_handler)(info);
        }
    }

    unsafe {
        info.prepare_for_ret().unwrap();
    }
}

/// Адрес обработчика `trap_handler()`, установленный с помощью [`syscall::set_trap_handler()`].
static TRAP_HANDLER: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
