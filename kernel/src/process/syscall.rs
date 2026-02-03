use alloc::vec::Vec;
use core::{
    arch::{
        asm,
        naked_asm,
    },
    str,
};

use super::registers::Registers;

use x86_64::registers::model_specific::{
    Efer,
    EferFlags,
    LStar,
    SFMask,
    Star,
};

use ku::{
    log::{
        self,
        Level,
        event,
    },
    process::{
        ExitCode,
        MiniContext,
        RFlags,
        ResultCode,
        State,
        Syscall,
    },
    sync::spinlock::SpinlockGuard,
};

use crate::{
    allocator::MemoryAllocator,
    error::{
        Error::{
            InvalidAlignment,
            InvalidArgument,
            NoPage,
            Overflow,
            PermissionDenied,
        },
        Result,
    },
    gdt::Gdt,
    log::{
        debug,
        error,
        info,
        trace,
        warn,
    },
    memory::{
        self,
        Block,
        FrameGuard,
        KERNEL_RW,
        Page,
        Translate,
        USER_R,
        Virt,
        mmu::{
            PageTableEntry,
            PageTableFlags,
        },
    },
    smp::{
        Cpu,
        KERNEL_RSP_OFFSET_IN_CPU,
    },
};

use super::{
    Pid,
    Process,
    Scheduler,
    Table,
    TrapContext,
};

use lock_set::{
    lock_dst,
    lock_src_dst,
};

// Used in docs.
#[allow(unused)]
use crate::error::Error;

/// Инициализация системных вызовов.
/// Подготавливает процессор к выполнению инструкций
/// [syscall](https://www.felixcloutier.com/x86/syscall) и
/// [sysret](https://www.felixcloutier.com/x86/sysret).
pub(crate) fn init() {
    let syscall_virt = Virt::from_ptr(syscall_trampoline as *const ());
    debug!(%syscall_virt);

    unsafe {
        let mut efer = Efer::read();
        efer |= EferFlags::SYSTEM_CALL_EXTENSIONS;
        Efer::write(efer);
    }

    Star::write(
        Gdt::user_code(),
        Gdt::user_data(),
        Gdt::kernel_code(),
        Gdt::kernel_data(),
    ).unwrap();

    LStar::write(syscall_virt.into());

    use x86_64::registers::rflags::RFlags as X86RFlags;
    SFMask::write(X86RFlags::all());
}

/// Получает управление при выполнении инструкции
/// [syscall](https://www.felixcloutier.com/x86/syscall).
///
/// Переключает стек на стек ядра, разрешает прерывания и
/// передаёт управление в функцию [`syscall()`].
#[unsafe(naked)]
extern "C" fn syscall_trampoline() -> ! {
    naked_asm!(
        "
            mov r15, rsp    
            mov rsp, gs:[{rsp_offset}]    // Load kernel stack
            sti

            sub rsp, 8                    
            push r15 
            push rcx
            push rax 
            
            mov rcx, r10

            call {syscall}
        ",

        rsp_offset = const KERNEL_RSP_OFFSET_IN_CPU,
        syscall = sym syscall,
    );
}

// ANCHOR: syscall
/// Выполняет диспетчеризацию системных вызовов по аргументу `number` --- номеру системного вызова.
///
/// Передаёт в функции, реализующие конкретные системные вызовы,
/// нужную часть аргументов `arg0`--`arg4`.
/// После выполнения функции конкретного системного вызова,
/// с помощью функции [`sysret()`] возвращает управление в контекст пользователя,
/// задаваемый `rip` и `rsp`.
extern "C" fn syscall(
    // https://wiki.osdev.org/System_V_ABI#x86-64:
    // Parameters to functions are passed in the registers
    // rdi, rsi, rdx, rcx, r8, r9,
    // and further values are passed on the stack in reverse order.
    arg0: usize,  // rdi
    arg1: usize,  // rsi
    arg2: usize,  // rdx
    arg3: usize,  // rcx
    arg4: usize,  // r8
    _arg5: usize, // r9
    // Stack, push in reverse order.
    number: usize,
    rip: Virt,
    rsp: Virt,
) -> ! {
    // ANCHOR_END: syscall

    assert!(
        RFlags::read().contains(RFlags::INTERRUPT_FLAG),
        "enable the interrupts during the system calls",
    );

    let syscall_result = Syscall::try_from(number);
    let context = MiniContext::new(rip, rsp);
    
    let cpu = Cpu::current_process();
    let process = match cpu {
        Ok(pid) => Table::get(pid),
        Err(_) => {
            error!("no current process during syscall");
            sysret(context, Err(crate::error::Error::NoProcess));
        }
    };

    match syscall_result {
        Ok(Syscall::Exit) => {
            exit(process.unwrap(), arg0);
        }
        Ok(Syscall::LogValue) => {
            let result = log_value(process.unwrap(), arg0, arg1, arg2, arg3);
            sysret(context, result);
        }
        Ok(Syscall::SchedYield) => {
            sched_yield(process.unwrap(), context);
        }
        Err(_) => {
            warn!(?syscall_result, %number, %arg0, %arg1, %arg2, %arg3, %arg4, "unknown syscall");
            sysret(context, Err(InvalidArgument));
        }
        _ => {
            warn!(?syscall_result, "unimplemented syscall");
            sysret(context, Err(crate::error::Error::Unimplemented));
        }
    };
}

// ANCHOR: sysret
/// С помощью инструкции [sysret](https://www.felixcloutier.com/x86/sysret)
/// возвращает управление в контекст пользователя `context`.
///
/// Передаёт пользователю результат системного вызова в виде кода успеха или ошибки и
/// полезной нагрузки из `result`.
fn sysret(
    context: MiniContext,
    result: Result<usize>,
) -> ! {
    // ANCHOR_END: sysret

    let (result_code, value) = match result {
        Ok(v) => (ResultCode::Ok, v),
        Err(ref e) => {
            let err_result: Result<()> = Err(e.clone());
            let code: ResultCode = err_result.into();
            (code, 0)
        }
    };

    let user_rsp = context.rsp().into_usize();
    let user_rip = context.rip().into_usize();
    let user_rflags = RFlags::default().into_usize();
    let ret_code = usize::from(result_code);
    
    unsafe {
        asm!(
            "
            mov r12, {rsp}
            mov rax, {result_code}
            mov rdi, {value}
            mov r11, {rflags}
            mov rcx, {rip}
            
            xor rbx, rbx
            xor rdx, rdx
            xor rsi, rsi
            xor rbp, rbp
            xor r8, r8
            xor r9, r9
            xor r10, r10
            xor r13, r13
            xor r14, r14
            xor r15, r15
            
            mov rsp, r12
            xor r12, r12

            sysretq
            ",

            result_code = in(reg) ret_code,
            value = in(reg) value,
            rflags = in(reg) user_rflags,
            rip = in(reg) user_rip,
            rsp = in(reg) user_rsp,
            options(noreturn),
        );
    }
}

// ANCHOR: exit
/// Выполняет системный вызов
/// [`lib::syscall::exit(code)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.exit.html).
///
/// Освобождает слот таблицы процессов и возвращается в контекст ядра,
/// из которого пользовательский процесс был запущен.
fn exit(
    process: SpinlockGuard<Process>,
    code: usize,
) -> ! {
    // ANCHOR_END: exit
    let pid = process.pid();
    let exit_code = ExitCode::try_from(code);
    
    info!(?pid, ?code, ?exit_code, "syscall = \"exit\"");
    
    memory::BASE_ADDRESS_SPACE.lock().switch_to();
    
    drop(process);
    Table::free(pid).expect("failed to free process in exit syscall");
    
    Cpu::set_current_process(None);

    unsafe {
        asm!(
            "mov rsp, gs:[{rsp_offset}]",
            "jmp {sched_yield}",
            rsp_offset = const KERNEL_RSP_OFFSET_IN_CPU,
            sched_yield = sym Registers::sched_yield,
            options(noreturn),
        );
    }
}

// ANCHOR: log_value
/// Выполняет системный вызов
/// [`lib::syscall::log_value(message, value)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.log_value.html).
///
/// Записывает в журнал строку `message` типа `&str`, заданную началом `start` и длиной `len`,
/// а также число `value`.
fn log_value(
    process: SpinlockGuard<Process>,
    level: usize,
    start: usize,
    len: usize,
    value: usize,
) -> Result<usize> {
    // ANCHOR_END: log_value
    let pid = process.pid();
    let level_char = level as u8 as char;
    let log_level = log::level_try_from_symbol(level_char).map_err(|_| InvalidArgument)?;
    let end = start.checked_add(len).ok_or(Overflow)?;

    let block = Block::<Virt>::from_index(start, end)?;

    let _checked_slice = process.lock_address_space().check_permission::<u8>(block, USER_R)?;
    let bytes = unsafe {
        core::slice::from_raw_parts(start as *const u8, len)
    };
    let message = str::from_utf8(bytes).map_err(|_| InvalidArgument)?;
    match log_level {
        Level::TRACE => trace!(%pid, %message, %value, hex_value = format_args!("{:#X}", value)),
        Level::DEBUG => debug!(%pid, %message, %value, hex_value = format_args!("{:#X}", value)),
        Level::INFO => info!(%pid, %message, %value, hex_value = format_args!("{:#X}", value)),
        Level::WARN => warn!(%pid, %message, %value, hex_value = format_args!("{:#X}", value)),
        Level::ERROR => error!(%pid, %message, %value, hex_value = format_args!("{:#X}", value)),
    }
    
    Ok(0)
}

/// Выполняет системный вызов
/// [`lib::syscall::sched_yield()`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.sched_yield.html).
///
/// Перепланирует процесс в конец очереди готовых к исполнению процессов и
/// забирает у него CPU функцией [`Process::sched_yield()`],
/// которая вернёт управление в другой контекст ядра ---
/// в контекст из которого была вызвана функция [`Process::enter_user_mode()`].
/// Текущий контекст исходного процесса --- `context` --- записывает в него,
/// чтобы в дальнейшем в него можно было вернуться через [`Process::enter_user_mode()`].
#[allow(unused_mut)] // TODO: remove before flight.
fn sched_yield(
    mut process: SpinlockGuard<Process>,
    context: MiniContext,
) -> ! {
    let pid = process.pid();
    
    info!(?pid, "syscall = \"sched_yield\"");
    
    process.set_context(context);
    
    Scheduler::enqueue(pid);
    
    memory::BASE_ADDRESS_SPACE.lock().switch_to();
    
    Cpu::set_current_process(None);
    
    drop(process);
    
    unsafe {
        asm!(
            "mov rsp, gs:[{rsp_offset}]",
            "jmp {sched_yield}",
            rsp_offset = const KERNEL_RSP_OFFSET_IN_CPU,
            sched_yield = sym Registers::sched_yield,
            options(noreturn),
        );
    }
}

// ANCHOR: exofork
/// Выполняет системный вызов
/// [`lib::syscall::exofork()`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.exofork.html).
///
/// Создаёт копию вызывающего процесса `process` и возвращает исходному процессу [`Pid`] копии.
/// Внутри копии возвращает [`Pid::Current`].
/// При этом новый процесс создаётся практически без адресного пространства и не готовый к работе.
/// Поэтому он, в частности, не ставится в очередь планировщика.
/// Текущий контекст исходного процесса --- `context` --- записывает в копию, чтобы в копии
/// вернуться туда же, куда происходит возврат из системного вызова для вызывающего процесса.
#[allow(unused_mut)] // TODO: remove before flight.
fn exofork(
    mut process: SpinlockGuard<Process>,
    context: MiniContext,
) -> Result<usize> {
    // ANCHOR_END: exofork
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: map
/// Выполняет системный вызов
/// [`lib::syscall::map(dst_pid, dst_block, flags)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.map.html).
///
/// Отображает в памяти процесса, заданного `dst_pid`, блок страниц размера `dst_size` байт
/// начиная с виртуального адреса `dst_address` с флагами доступа `flags`.
/// Если `dst_address` равен нулю,
/// сам выбирает свободный участок адресного пространства размера `dst_size`.
fn map(
    process: SpinlockGuard<Process>,
    dst_pid: usize,
    dst_address: usize,
    dst_size: usize,
    flags: usize,
) -> Result<usize> {
    // ANCHOR_END: map
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: unmap
/// Выполняет системный вызов
/// [`lib::syscall::unmap(dst_pid, dst_block)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.unmap.html).
///
/// Удаляет из виртуальной памяти целевого процесса `dst_pid` блок страниц
/// размера `dst_size` байт начиная с виртуального адреса `dst_address`.
fn unmap(
    process: SpinlockGuard<Process>,
    dst_pid: usize,
    dst_address: usize,
    dst_size: usize,
) -> Result<usize> {
    // ANCHOR_END: unmap
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: copy_mapping
/// Выполняет системный вызов
/// [`lib::syscall::copy_mapping(dst_pid, src_block, dst_block, flags)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.copy_mapping.html).
///
/// Создаёт копию отображения виртуальной памяти из вызывающего процесса `process`
/// в процесс, заданный `dst_pid`.
/// Исходный диапазон начинается с виртуального адреса `src_address`,
/// целевой --- с виртуального адреса `dst_address`.
/// Размер диапазона --- `dst_size` байт.
///
/// В целевом процессе диапазон должен быть отображён с флагами:
///   - `flags`, если `flags != 0`.
///   - такими же, как в исходном диапазоне, если `flags == 0`.
///
/// Не допускает целевое отображение с более широким набором флагов, чем исходное.
/// После выполнения у процессов появляется область
/// [разделяемой памяти](https://en.wikipedia.org/wiki/Shared_memory).
fn copy_mapping(
    process: SpinlockGuard<Process>,
    dst_pid: usize,
    src_address: usize,
    dst_address: usize,
    dst_size: usize,
    flags: usize,
) -> Result<usize> {
    // ANCHOR_END: copy_mapping
    // TODO: your code here.
    unimplemented!();
}

/// Проверяет, что заданный блок виртуальных страниц `block` отображён в
/// адресное пространство процесса `process` с корректно заданными флагами `flags`.
/// Возвращает вектор физических фреймов, в которые отображены эти страницы.
/// См. также [`check_frame()`].
fn check_frames<'a>(
    process: &'a SpinlockGuard<Process>,
    block: Block<Page>,
    flags: PageTableFlags,
) -> Result<Vec<(FrameGuard, PageTableFlags), MemoryAllocator<'a>>> {
    // TODO: your code here.
    unimplemented!();
}

/// Выполняет отображение `src_ptes` в `dst_pages`
/// в адресное пространство процесса `process`.
///
/// В целевом процессе диапазон должен быть отображён с флагами:
///   - `flags`, если `flags` --- [`Some`].
///   - такими же, как в исходном диапазоне, если `flags` --- [`None`].
///
/// Количества элементов в `src_ptes` и `dst_pages` должны совпадать.
fn map_pages_to_frames(
    process: &SpinlockGuard<Process>,
    src_ptes: Vec<(FrameGuard, PageTableFlags), MemoryAllocator>,
    dst_pages: Block<Page>,
    flags: Option<PageTableFlags>,
) -> Result<()> {
    assert_eq!(src_ptes.len(), dst_pages.count());

    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: set_state
/// Выполняет системный вызов
/// [`lib::syscall::set_state(dst_pid, state)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.set_state.html).
///
/// Переводит целевой процесс, заданный идентификатором `dst_pid`, в заданное состояние `state`.
/// Ставит его в очередь планировщика в случае [`State::Runnable`].
fn set_state(
    process: SpinlockGuard<Process>,
    dst_pid: usize,
    state: usize,
) -> Result<usize> {
    // ANCHOR_END: set_state
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: set_trap_handler
/// Выполняет системный вызов
/// [`lib::syscall::set_trap_handler(dst_pid, trap_handler, trap_stack)`](https://sergey-v-galtsev.gitlab.io/labs-description/doc/lib/syscall/fn.set_trap_handler.html).
///
/// Устанавливает для целевого процесса, заданного идентификатором `dst_pid`,
/// пользовательский обработчик прерывания с виртуальным адресом `rip` и стеком,
/// который задаётся блоком виртуальных адресов начиная с `stack_address` и размера `stack_size`.
/// Стек может быть не выровнен по границе страниц.
fn set_trap_handler(
    process: SpinlockGuard<Process>,
    dst_pid: usize,
    rip: usize,
    stack_address: usize,
    stack_size: usize,
) -> Result<usize> {
    // ANCHOR_END: set_trap_handler
    // TODO: your code here.
    unimplemented!();
}

/// Проверяет, что `address` и `size` задают корректно выровненный диапазон страниц,
/// целиком лежащий внутри одной из
/// [двух непрерывных половин](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
/// адресного пространства.
/// Возвращает блок соответствующих виртуальных страниц.
fn check_block(
    address: usize,
    size: usize,
) -> Result<Block<Page>> {
    // TODO: your code here.
    unimplemented!();
}

/// Проверяет, что заданная виртуальная страница `page` отображена в
/// адресное пространство процесса `process` с заданными флагами `flags`.
/// Флаги [`PageTableFlags::COPY_ON_WRITE`] и [`PageTableFlags::WRITABLE`]
/// при проверке считаются эквивалентными,
/// а флаг [`PageTableFlags::USER`] --- обязательно включённым.
/// Возвращает копию [`PageTableEntry`], с физическим фреймом,
/// в который она отображена, и флагами исходного отображения.
///
/// Возвращает ошибки:
///   - [`Error::NoPage`] если страница `page` не отображена.
///   - [`Error::PermissionDenied`] если страница отображена,
///     но не со всеми запрошенными флагами.
fn check_frame(
    process: &SpinlockGuard<Process>,
    page: Page,
    flags: PageTableFlags,
) -> Result<PageTableEntry> {
    // TODO: your code here.
    unimplemented!();
}

/// Проверяет, что `flags` задаёт валидный набор флагов отображения страниц пользователя.
///
/// Возвращает:
///   - [`None`], если `flags == 0`.
///   - Входные `flags` в виде [`PageTableFlags`], если `flags != 0`.
///
/// Возвращает ошибки:
///   - [`Error::InvalidArgument`], если во `flags` установлен бит,
///     не соответствующий никакому флагу [`PageTableFlags`].
///   - [`Error::PermissionDenied`], если `flags != 0` и
///     в них не включён [`PageTableFlags::USER`].
fn check_page_flags(flags: usize) -> Result<Option<PageTableFlags>> {
    // TODO: your code here.
    unimplemented!();
}

/// Работа с блокировкой одного процесса или парой блокировок двух разных процессов.
mod lock_set {
    use duplicate::duplicate_item;

    use ku::{
        error::{
            Error::{
                NoProcess,
                PermissionDenied,
            },
            Result,
        },
        process::Pid,
        sync::spinlock::SpinlockGuard,
    };

    use super::super::{
        Process,
        Table,
    };

    /// Проверяет, что процесс `src` имеет право модифицировать целевой процесс,
    /// заданный своим идентификатором `dst_pid`.
    /// Целевой процесс может совпадать с `src`.
    ///
    /// Модифицировать можно:
    ///   - Либо самого себя, задавая [`Pid::Current`] или явно собственный идентификатор [`Pid::Id`].
    ///   - Либо свой непосредственно дочерний процесс, задавая его идентификатор.
    ///
    /// Возвращает блокировку на процесс `dst`.
    pub(super) fn lock_dst(
        src: SpinlockGuard<Process>,
        dst_pid: usize,
    ) -> Result<LockSet> {
        LockSet::new(src, dst_pid, ProcessSet::Dst)
    }

    /// Проверяет, что процесс `src` имеет право модифицировать целевой процесс,
    /// заданный своим идентификатором `dst_pid`.
    /// Целевой процесс может совпадать с `src`.
    ///
    /// Модифицировать можно:
    ///   - Либо самого себя, задавая [`Pid::Current`] или явно собственный идентификатор [`Pid::Id`].
    ///   - Либо свой непосредственно дочерний процесс, задавая его идентификатор.
    ///
    /// Возвращает:
    ///   - Исходную блокировку на `src`, если `dst_pid` задаёт тот же процесс.
    ///   - Блокировки на процессы `src` и `dst`, если это разные процессы.
    ///
    /// Захватывает блокировки в правильном порядке для избежания
    /// [взаимоблокировки](https://en.wikipedia.org/wiki/Deadlock).
    pub(super) fn lock_src_dst(
        src: SpinlockGuard<Process>,
        dst_pid: usize,
    ) -> Result<LockSet> {
        LockSet::new(src, dst_pid, ProcessSet::SrcDst)
    }

    #[derive(Debug)]
    /// Блокировка одного процесса или пара блокировок двух разных процессов.
    pub(super) enum LockSet<'a> {
        /// Пара блокировок двух разных процессов.
        Different {
            /// Процесс, над которым совершается действие системного вызова.
            dst: SpinlockGuard<'a, Process>,

            /// Процесс, запустивший системный вызов.
            src: SpinlockGuard<'a, Process>,
        },

        /// Блокировка только процесса, над которым совершается действие системного вызова.
        /// Используется когда не нужна блокировка на процесс, запустивший системный вызов.
        Dst {
            /// Процесс, над которым совершается действие системного вызова.
            dst: SpinlockGuard<'a, Process>,
        },

        /// Блокировка одного процесса, который и запустил системный вызов
        /// и одновременно является целевым процессом для этого системного вызова.
        Same {
            /// Процесс, запустивший системный вызов на себя же.
            src_dst: SpinlockGuard<'a, Process>,
        },
    }

    impl<'a> LockSet<'a> {
        // ANCHOR: lock_set
        /// Проверяет, что процесс `src` имеет право модифицировать целевой процесс,
        /// заданный своим идентификатором `dst_pid`.
        /// Целевой процесс может совпадать с `src`.
        ///
        /// Модифицировать можно:
        ///   - Либо самого себя, задавая [`Pid::Current`] или явно собственный идентификатор [`Pid::Id`].
        ///   - Либо свой непосредственно дочерний процесс, задавая его идентификатор.
        ///
        /// Возвращает:
        ///   - Исходную блокировку на `src`, если `dst_pid` задаёт тот же процесс.
        ///   - Блокировку на процесс `dst`, если `process_set == ProcessSet::Dst`.
        ///   - Блокировки на процессы `src` и `dst`, если это разные процессы.
        ///
        /// Захватывает блокировки в правильном порядке для избежания
        /// [взаимоблокировки](https://en.wikipedia.org/wiki/Deadlock).
        fn new(
            src: SpinlockGuard<'_, Process>,
            dst_pid: usize,
            process_set: ProcessSet,
        ) -> Result<LockSet<'_>> {
            // ANCHOR_END: lock_set
            // TODO: your code here.
            unimplemented!();
        }

        /// Возвращает процесс, над которым совершается действие системного вызова.
        #[allow(clippy::needless_arbitrary_self_type)]
        #[duplicate_item(
            dst_accessor reference(type);
            [dst] [&'b type];
            [dst_mut] [&'b mut type];
        )]
        pub(super) fn dst_accessor<'b>(
            self: reference([Self])
        ) -> reference([SpinlockGuard<'a, Process>]) {
            match self {
                LockSet::Same { src_dst } => src_dst,
                LockSet::Different { dst, .. } => dst,
                LockSet::Dst { dst } => dst,
            }
        }

        /// Возвращает процесс, запустивший системный вызов.
        ///
        /// # Panics
        ///
        /// Паникует, если изначально блокировка захватывалась только на целевой процесс.
        #[allow(clippy::needless_arbitrary_self_type)]
        #[allow(dead_code)]
        #[duplicate_item(
            src_accessor reference(type);
            [src] [&'b type];
            [src_mut] [&'b mut type];
        )]
        pub(super) fn src_accessor<'b>(
            self: reference([Self])
        ) -> reference([SpinlockGuard<'a, Process>]) {
            match self {
                LockSet::Same { src_dst } => src_dst,
                LockSet::Different { src, .. } => src,
                LockSet::Dst { .. } => panic!("only destination process is locked"),
            }
        }

        /// Возвращает `true`, если `src` и `dst` --- это один и тот же процесс.
        ///
        /// # Panics
        ///
        /// Паникует, если изначально блокировка захватывалась только на целевой процесс.
        #[allow(dead_code)]
        pub(super) fn is_same(&self) -> bool {
            match self {
                LockSet::Same { .. } => true,
                LockSet::Different { .. } => false,
                LockSet::Dst { .. } => panic!("only destination process is locked"),
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Указывает какой набор процессов блокировать.
    enum ProcessSet {
        /// Блокировать только процесс, над которым совершается действие системного вызова.
        Dst,

        /// Блокировать оба процесса ---
        /// и процесс, запустивший системный вызов,
        /// и процесс, над которым совершается действие системного вызова.
        SrcDst,
    }
}

#[doc(hidden)]
pub mod test_scaffolding {
    use ku::{
        process::MiniContext,
        sync::spinlock::SpinlockGuard,
    };

    use crate::error::Result;

    use super::super::Process;

    pub fn log_value(
        process: SpinlockGuard<Process>,
        level: usize,
        start: usize,
        len: usize,
        value: usize,
    ) -> Result<usize> {
        super::log_value(process, level, start, len, value)
    }

    pub fn exofork(process: SpinlockGuard<Process>) -> Result<usize> {
        super::exofork(process, MiniContext::default())
    }

    pub fn map(
        process: SpinlockGuard<Process>,
        dst_pid: usize,
        dst_address: usize,
        dst_size: usize,
        flags: usize,
    ) -> Result<usize> {
        super::map(process, dst_pid, dst_address, dst_size, flags)
    }

    pub fn unmap(
        process: SpinlockGuard<Process>,
        dst_pid: usize,
        dst_address: usize,
        dst_size: usize,
    ) -> Result<usize> {
        super::unmap(process, dst_pid, dst_address, dst_size)
    }

    pub fn copy_mapping(
        process: SpinlockGuard<Process>,
        dst_pid: usize,
        src_address: usize,
        dst_address: usize,
        dst_size: usize,
        flags: usize,
    ) -> Result<usize> {
        super::copy_mapping(process, dst_pid, src_address, dst_address, dst_size, flags)
    }

    pub fn set_state(
        process: SpinlockGuard<Process>,
        dst_pid: usize,
        state: usize,
    ) -> Result<usize> {
        super::set_state(process, dst_pid, state)
    }
}
