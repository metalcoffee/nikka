use core::{
    arch::naked_asm,
    fmt,
    mem,
    ops::Index,
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

use lazy_static::lazy_static;
use x86_64::{
    VirtAddr,
    instructions::{
        interrupts,
        tables,
    },
    structures::{
        DescriptorTablePointer,
        idt::{
            Entry,
            EntryOptions,
        },
    },
};

use ku::{
    backtrace::Backtrace,
    process::Info,
    sync::{
        self,
        spinlock::Spinlock,
    },
};

#[cfg(not(feature = "conservative-backtraces"))]
use sentinel_frame::with_sentinel_frame;

use crate::{
    fs::BlockCache,
    log::{
        error,
        info,
        warn,
    },
    memory::{
        DOUBLE_FAULT_IST_INDEX,
        PAGE_FAULT_IST_INDEX,
        Virt,
    },
    process::{
        ModeContext,
        Pid,
        Process,
        Table,
    },
    smp::{
        Cpu,
        LocalApic,
    },
    time::{
        pit8254,
        rtc,
    },
};

pub use ku::process::Trap;

/// Первое прерывание
/// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
/// [Стандартная последовательность](https://wiki.osdev.org/Interrupts#Standard_ISA_IRQs)
/// подключения
/// [прерываний в x86](https://en.wikipedia.org/wiki/Interrupt_request_(PC_architecture))
/// для устройств шины
/// [Industry Standard Architecture](https://en.wikipedia.org/wiki/Industry_Standard_Architecture)
/// (ISA).
const PIC_BASE: usize = Trap::Pit as usize;

/// Количество исключений и прерываний.
const COUNT: usize = Trap::Spurious as usize + 1;

// ANCHOR: statistics
/// Информация о прерывании.
pub struct Statistics {
    /// Сколько раз сработало это прерывание.
    count: AtomicUsize,

    /// Короткая мнемоника прерывания.
    mnemonic: &'static str,

    /// Имя прерывания.
    name: &'static str,
}
// ANCHOR_END: statistics

impl Statistics {
    /// Создаёт информацию о прерывании с именем `name` и короткой мнемоникой `mnemonic`.
    const fn new(
        name: &'static str,
        mnemonic: &'static str,
    ) -> Statistics {
        Statistics {
            name,
            mnemonic,
            count: AtomicUsize::new(0),
        }
    }

    /// Сколько раз сработало это прерывание.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// Короткая мнемоника прерывания.
    pub fn mnemonic(&self) -> &'static str {
        self.mnemonic
    }

    /// Имя прерывания.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Инкрементирует счётчик срабатывания прерывания.
    fn inc(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Информация обо всех прерываниях.
pub struct TrapStats([Statistics; COUNT]);

impl TrapStats {
    /// Возвращает итератор по статистикам прерываний.
    pub fn iter(&self) -> core::slice::Iter<'_, Statistics> {
        self.0.iter()
    }
}

impl Index<Trap> for TrapStats {
    type Output = Statistics;

    fn index(
        &self,
        index: Trap,
    ) -> &Self::Output {
        &self.0[usize::from(index)]
    }
}

/// Информация обо всех прерываниях.
pub static TRAP_STATS: TrapStats = TrapStats([
    Statistics::new("Divide Error", "#DE"),
    Statistics::new("Debug", "#DB"),
    Statistics::new("Non-maskable Interrupt", "#NM"),
    Statistics::new("Breakpoint", "#BP"),
    Statistics::new("Overflow", "#OF"),
    Statistics::new("Bound Range Exceeded", "#BR"),
    Statistics::new("Invalid Opcode", "#UD"),
    Statistics::new("Device Not Available", "#NA"),
    Statistics::new("Double Fault", "#DF"),
    Statistics::new("Coprocessor Segment Overrun", "#CS"),
    Statistics::new("Invalid TSS", "#TS"),
    Statistics::new("Segment Not Present", "#NP"),
    Statistics::new("Stack-Segment Fault", "#SS"),
    Statistics::new("General Protection Fault", "#GP"),
    Statistics::new("Page Fault", "#PF"),
    Statistics::new("Reserved 0x0F", "#0F"),
    Statistics::new("x87 Floating-Point Exception", "#MF"),
    Statistics::new("Alignment Check", "#AC"),
    Statistics::new("Machine Check", "#MC"),
    Statistics::new("SIMD Floating-Point Exception", "#XF"),
    Statistics::new("Virtualization Exception", "#VE"),
    Statistics::new("Reserved 0x15", "#15"),
    Statistics::new("Reserved 0x16", "#16"),
    Statistics::new("Reserved 0x17", "#17"),
    Statistics::new("Reserved 0x18", "#18"),
    Statistics::new("Reserved 0x19", "#19"),
    Statistics::new("Reserved 0x1A", "#1A"),
    Statistics::new("Reserved 0x1B", "#1B"),
    Statistics::new("Reserved 0x1C", "#1C"),
    Statistics::new("Reserved 0x1D", "#1D"),
    Statistics::new("Security Exception", "#SX"),
    Statistics::new("Reserved 0x1F", "#1F"),
    Statistics::new("PIT", "#IT"),
    Statistics::new("Keyboard", "#KB"),
    Statistics::new("Cascade", "#CA"),
    Statistics::new("COM2", "#C2"),
    Statistics::new("COM1", "#C1"),
    Statistics::new("LPT2", "#L2"),
    Statistics::new("Floppy Disk", "#FD"),
    Statistics::new("LPT1", "#L1"),
    Statistics::new("RTC", "#RT"),
    Statistics::new("Free 0x29", "#29"),
    Statistics::new("Free 0x2A", "#2A"),
    Statistics::new("Free 0x2B", "#2B"),
    Statistics::new("PS2 Mouse", "#MS"),
    Statistics::new("Coprocessor", "#CP"),
    Statistics::new("Primary ATA Hard Disk", "#PD"),
    Statistics::new("Secondary ATA Hard Disk", "#SD"),
    Statistics::new("Timer", "#TI"),
    Statistics::new("Spurious", "#SP"),
]);

// ANCHOR: init
/// Инициализирует таблицу обработчиков прерываний [`struct@IDT`].
pub(super) fn init() {
    unsafe {
        pic8259::init(PIC_BASE.try_into().expect("too many interrupt numbers"));
    }

    IDT.load();

    interrupts::enable();

    rtc::enable_next_interrupt();

    info!("traps init");
}
// ANCHOR_END: init

/// A helper to generate the code for exception handlers.
macro_rules! exception_with_error_code {
    ($name:ident, $trap:expr) => {
        #[unsafe(naked)]
        extern "C" fn $name() {
            naked_asm!(
                "
                push {number}
                jmp {trap_trampoline}
                ",
                trap_trampoline = sym trap_trampoline,
                number = const $trap as usize,
            )
        }
    };

    ($idt: ident, $name:ident, $trap:expr) => {
        {
            exception_with_error_code!($name, $trap);
            $idt.get_mut($trap).set_trap_handler($name);
        }
    };

    ($idt: ident, $name:ident, $trap:expr, $stack:expr) => {
        {
            exception_with_error_code!($name, $trap);
            $idt.get_mut($trap)
                .set_trap_handler($name)
                .set_stack_index($stack);
        }
    };
}

/// A helper to generate the code for exception handlers.
macro_rules! exception_without_error_code {
    ($name:ident, $trap:expr) => {
        #[unsafe(naked)]
        extern "C" fn $name() {
            naked_asm!(
                "
                push {error_code}
                push {number}
                jmp {trap_trampoline}
                ",
                error_code = const 0,
                trap_trampoline = sym trap_trampoline,
                number = const $trap as usize,
            )
        }
    };

    ($idt: ident, $name:ident, $trap:expr) => {
        {
            exception_without_error_code!($name, $trap);
            $idt.get_mut($trap).set_trap_handler($name);
        }
    };
}

/// Таблица обработчиков прерываний
/// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT).
pub(crate) struct Idt([IdtEntry; COUNT]);

impl Idt {
    /// Создаёт таблицу обработчиков прерываний
    /// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT).
    fn new() -> Self {
        let mut idt = Self([IdtEntry::missing(); COUNT]);

        unsafe {
            exception_with_error_code!(
                idt,
                double_fault,
                Trap::DoubleFault,
                DOUBLE_FAULT_IST_INDEX
            );
            exception_with_error_code!(idt, page_fault, Trap::PageFault, PAGE_FAULT_IST_INDEX);
        }

        exception_without_error_code!(idt, divide_error, Trap::DivideError);
        exception_without_error_code!(idt, debug, Trap::Debug);
        exception_without_error_code!(idt, non_maskable_interrupt, Trap::NonMaskableInterrupt);
        exception_without_error_code!(idt, breakpoint, Trap::Breakpoint);
        exception_without_error_code!(idt, overflow, Trap::Overflow);
        exception_without_error_code!(idt, bound_range_exceeded, Trap::BoundRangeExceeded);
        exception_without_error_code!(idt, invalid_opcode, Trap::InvalidOpcode);
        exception_without_error_code!(idt, device_not_available, Trap::DeviceNotAvailable);
        exception_with_error_code!(idt, invalid_tss, Trap::InvalidTss);
        exception_with_error_code!(idt, segment_not_present, Trap::SegmentNotPresent);
        exception_with_error_code!(idt, stack_segment_fault, Trap::StackSegmentFault);
        exception_with_error_code!(idt, general_protection_fault, Trap::GeneralProtectionFault);
        exception_without_error_code!(idt, x87_floating_point, Trap::X87FloatingPoint);
        exception_with_error_code!(idt, alignment_check, Trap::AlignmentCheck);
        exception_without_error_code!(idt, machine_check, Trap::MachineCheck);
        exception_without_error_code!(idt, simd_floating_point, Trap::SimdFloatingPoint);
        exception_without_error_code!(idt, virtualization, Trap::Virtualization);
        exception_with_error_code!(idt, security_exception, Trap::SecurityException);

        idt.get_mut(Trap::Pit).set_handler(pit);
        idt.get_mut(Trap::Keyboard).set_handler(keyboard);
        idt.get_mut(Trap::Cascade).set_handler(cascade);
        idt.get_mut(Trap::Com2).set_handler(com2);
        idt.get_mut(Trap::Com1).set_handler(com1);
        idt.get_mut(Trap::Lpt2).set_handler(lpt2);
        idt.get_mut(Trap::FloppyDisk).set_handler(floppy_disk);
        idt.get_mut(Trap::Lpt1).set_handler(lpt1);
        idt.get_mut(Trap::Rtc).set_handler(rtc);
        idt.get_mut(Trap::Free29).set_handler(free_29);
        idt.get_mut(Trap::Free2A).set_handler(free_2a);
        idt.get_mut(Trap::Free2B).set_handler(free_2b);
        idt.get_mut(Trap::Ps2Mouse).set_handler(ps2_mouse);
        idt.get_mut(Trap::Coprocessor).set_handler(coprocessor);
        idt.get_mut(Trap::Ata0).set_handler(ata0);
        idt.get_mut(Trap::Ata1).set_handler(ata1);
        idt.get_mut(Trap::Timer).set_handler(timer);
        idt.get_mut(Trap::Spurious).set_handler(spurious);

        idt
    }

    /// Загружает дескриптор таблицы прерываний
    /// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT)
    /// в регистр [`IDTR`](https://wiki.osdev.org/Interrupt_Descriptor_Table#IDTR).
    pub(crate) fn load(&self) {
        unsafe {
            let pseudo_descriptor = DescriptorTablePointer {
                base: Virt::from_ref(&self.0).into(),
                limit: (mem::size_of_val(&self.0) - 1).try_into().expect("the IDT is too large"),
            };
            tables::lidt(&pseudo_descriptor);
        }
    }

    /// Возвращает запись таблицы обработчиков прерываний для прерывания `trap`.
    fn get_mut(
        &mut self,
        index: Trap,
    ) -> &mut IdtEntry {
        &mut self.0[usize::from(index)]
    }
}

lazy_static! {
    /// Таблица обработчиков прерываний
    /// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT).
    pub(crate) static ref IDT: Idt = Idt::new();
}

/// Обёртка для [`ModeContext`].
/// Она делает запись в него через [`core::ptr::write_volatile()`],
/// чтобы компилятор такую операцию записи не выкинул.
#[allow(rustdoc::private_intra_doc_links)]
#[repr(transparent)]
pub struct TrapContext(ModeContext);

#[allow(rustdoc::private_intra_doc_links)]
impl TrapContext {
    /// Возвращает `true`, если контекст имеет привилегии пользователя.
    pub fn is_user_mode(&self) -> bool {
        self.0.is_user_mode()
    }

    /// Возвращает [`ModeContext`], содержащийся в этом [`TrapContext`].
    pub fn get(&self) -> ModeContext {
        self.0
    }

    /// Записывает `context` в этот [`TrapContext`].
    ///
    /// Делает это через [`core::ptr::write_volatile()`],
    /// чтобы компилятор такую операцию записи не выкинул.
    pub fn set(
        &mut self,
        context: ModeContext,
    ) {
        unsafe {
            (&mut self.0 as *mut ModeContext).write_volatile(context);
        }
    }
}

impl fmt::Debug for TrapContext {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{:?}", self.get())
    }
}

impl fmt::Display for TrapContext {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}", self.get())
    }
}

/// Тип функции обработчиков прерываний.
type InterruptHandler = fn(TrapContext);

/// Запись таблицы прерываний
/// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT).
#[derive(Clone, Copy)]
struct IdtEntry(Entry<InterruptHandler>);

impl IdtEntry {
    /// Создаёт запись таблицы прерываний без обработчика.
    const fn missing() -> Self {
        Self(Entry::missing())
    }

    /// Устанавливает обработчик `handler` исключения процессора.
    fn set_trap_handler(
        &mut self,
        handler: extern "C" fn(),
    ) -> &mut EntryOptions {
        unsafe { self.0.set_handler_addr(VirtAddr::from_ptr(handler as *const ())) }
    }

    /// Устанавливает обработчик `handler` прерывания.
    fn set_handler(
        &mut self,
        handler: extern "x86-interrupt" fn(TrapContext),
    ) -> &mut EntryOptions {
        unsafe { self.0.set_handler_addr(VirtAddr::from_ptr(handler as *const ())) }
    }
}

/// Общий обработчик исключений и прерываний.
///
/// - `number` --- номер прерывания, также определяет фатальность исключения.
/// - `error_code` --- дополнительная информация для некоторых исключений процессора.
/// - `context` --- контекст в котором возникло прерывание.
/// - `rpb` --- значение регистра `rbp` в контексте прерывания для построения [`Backtrace`].
///
/// Если фатальное исключение вызвал процесс, то он будет остановлен и удалён.
/// Если фатальное исключение вызвало ядро, оно запаникует.
#[cfg_attr(not(feature = "conservative-backtraces"), with_sentinel_frame)]
extern "C" fn generic_trap(
    number: usize,             // rdi
    error_code: usize,         // rsi
    context: &mut TrapContext, // rdx
    rbp: usize,                // rcx
) {
    let trap = Trap::try_from(number).expect("unexpected trap number");

    if trap == Trap::DoubleFault {
        mem::forget(STOP_ALL_CPUS.lock());
        sync::start_panicking();
    }

    TRAP_STATS[trap].inc();

    let fatal = trap != Trap::Breakpoint && trap != Trap::Overflow;
    let info = Info::new(trap, error_code);

    if context.is_user_mode() {
        let pid = Cpu::current_process().expect("user mode without a process");
        if pid == Pid::Current {
            context.set(ModeContext::kernel_context(Virt::default()));
            return;
        }
        let mut process =
            Table::get(pid).expect("failed to find the current process in the process table");

        if process.trap(context, trap, info) {
            return;
        }

        info!(
            trap = TRAP_STATS[trap].name,
            number,
            %info,
            %context,
            %pid,
            "user mode trap",
        );

        if fatal {
            drop(process);
            if let Err(error) = Table::free(pid) {
                warn!(
                    %pid,
                    ?error,
                    "failed to free the process, maybe it was destroyed concurrently",
                );
            }
            Process::sched_yield();
        }
    } else {
        match BlockCache::trap_handler(&info) {
            Ok(true) => return,
            Ok(false) => {},
            Err(error) => error!(?error, "failed to handle a page fault in the block cache"),
        }

        let backtrace =
            Backtrace::with_context(rbp, context.get().mini_context()).unwrap_or_default();

        if fatal {
            panic!(
                "kernel mode trap #{} - {}, context: {}, info: {}, backtrace: {}",
                number, TRAP_STATS[trap].name, context, info, backtrace,
            );
        } else {
            error!(
                trap = TRAP_STATS[trap].name,
                number,
                %context,
                %info,
                %backtrace,
                "kernel mode trap",
            );
        }
    }
}

/// Общий трамплин для всех исключений процессора.
#[unsafe(naked)]
extern "C" fn trap_trampoline() {
    naked_asm!(
        "
        // System V ABI for x86-64 caller-saved registers.
        push rax
        push rcx
        push rdx
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11

        // The trap number is right above the caller-saved registers.
        mov rdi, [rsp + {caller_saved_registers_size}]

        // Next is the error code.
        mov rsi, [rsp + {caller_saved_registers_size} + {trap_number_size}]

        // Next is the interrupt context.
        lea rdx, [rsp + {caller_saved_registers_size} + {trap_number_size} + {error_code_size}]

        mov rcx, rbp

        call {generic_trap}

        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rdx
        pop rcx
        pop rax

        add rsp, {trap_number_size} + {error_code_size}

        iretq
        ",

        caller_saved_registers_size = const 9 * mem::size_of::<usize>(),
        error_code_size = const mem::size_of::<usize>(),
        generic_trap = sym generic_trap,
        trap_number_size = const mem::size_of::<usize>(),
    )
}

// ANCHOR: generic_pic_interrupt
/// Выполняет общую часть обработки для всех прерываний
/// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
/// Аргумент `number` задаёт номер прерывания в общей нумерации таблицы обработчиков прерываний
/// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT).
fn generic_pic_interrupt(number: Trap) {
    TRAP_STATS[number].inc();
    unsafe {
        pic8259::end_of_interrupt(usize::from(number) - PIC_BASE);
    }
}
// ANCHOR_END: generic_pic_interrupt

/// Обработчик прерывания таймера [Intel 8253/8254](https://en.wikipedia.org/wiki/Intel_8253)
/// ([programmable interval timer, PIT](https://en.wikipedia.org/wiki/Programmable_interval_timer)).
extern "x86-interrupt" fn pit(_context: TrapContext) {
    pit8254::interrupt();
    generic_pic_interrupt(Trap::Pit);
}

/// Обработчик прерывания клавиатуры.
extern "x86-interrupt" fn keyboard(_context: TrapContext) {
    generic_pic_interrupt(Trap::Keyboard);
}

/// Обработчик каскадного прерывания первого контроллера
/// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259),
/// к которому подключён второй такой же.
extern "x86-interrupt" fn cascade(_context: TrapContext) {
    generic_pic_interrupt(Trap::Cascade);
}

/// Обработчик прерывания
/// [последовательных портов](https://en.wikipedia.org/wiki/Serial_port) номер 2 и 4.
extern "x86-interrupt" fn com2(_context: TrapContext) {
    generic_pic_interrupt(Trap::Com2);
}

/// Обработчик прерывания
/// [последовательных портов](https://en.wikipedia.org/wiki/Serial_port) номер 1 и 3.
extern "x86-interrupt" fn com1(_context: TrapContext) {
    generic_pic_interrupt(Trap::Com1);
}

/// Обработчик прерывания второго параллельного порта
/// ([Parallel port](https://en.wikipedia.org/wiki/Parallel_port)).
/// Так как через параллельные порты чаще всего подключались принтеры
/// ([Line printer](https://en.wikipedia.org/wiki/Line_printer)),
/// сохранилось их сокращение LPT.
extern "x86-interrupt" fn lpt2(_context: TrapContext) {
    generic_pic_interrupt(Trap::Lpt2);
}

/// Обработчик прерывания контроллера [дискет](https://en.wikipedia.org/wiki/Floppy_disk).
extern "x86-interrupt" fn floppy_disk(_context: TrapContext) {
    generic_pic_interrupt(Trap::FloppyDisk);
}

/// Обработчик прерывания первого и третьего параллельного порта
/// ([Parallel port](https://en.wikipedia.org/wiki/Parallel_port)).
/// Так как через параллельные порты чаще всего подключались принтеры
/// ([Line printer](https://en.wikipedia.org/wiki/Line_printer)),
/// сохранилось их сокращение LPT.
extern "x86-interrupt" fn lpt1(_context: TrapContext) {
    generic_pic_interrupt(Trap::Lpt1);
}

// ANCHOR: rtc
/// Обработчик прерываний
/// [часов реального времени (Real-time clock, RTC)](https://en.wikipedia.org/wiki/Real-time_clock).
extern "x86-interrupt" fn rtc(_context: TrapContext) {
    rtc::interrupt();
    generic_pic_interrupt(Trap::Rtc);
}
// ANCHOR_END: rtc

/// Обработчик прерывания входа `0x9` каскадной пары
/// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
extern "x86-interrupt" fn free_29(_context: TrapContext) {
    generic_pic_interrupt(Trap::Free29);
}

/// Обработчик прерывания входа `0xA` каскадной пары
/// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
extern "x86-interrupt" fn free_2a(_context: TrapContext) {
    generic_pic_interrupt(Trap::Free2A);
}

/// Обработчик прерывания входа `0xB` каскадной пары
/// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
extern "x86-interrupt" fn free_2b(_context: TrapContext) {
    generic_pic_interrupt(Trap::Free2B);
}

/// Обработчик прерывания мыши.
extern "x86-interrupt" fn ps2_mouse(_context: TrapContext) {
    generic_pic_interrupt(Trap::Ps2Mouse);
}

/// Обработчик прерывания сопроцессора.
extern "x86-interrupt" fn coprocessor(_context: TrapContext) {
    generic_pic_interrupt(Trap::Coprocessor);
}

/// Обработчик прерывания первого контроллера
/// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA).
extern "x86-interrupt" fn ata0(_context: TrapContext) {
    generic_pic_interrupt(Trap::Ata0);
}

/// Обработчик прерывания второго контроллера
/// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA).
extern "x86-interrupt" fn ata1(_context: TrapContext) {
    generic_pic_interrupt(Trap::Ata1);
}

/// Выполняет общую часть обработки для всех прерываний
/// [APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller#APIC_timer).
/// Аргумент `number` задаёт номер прерывания в общей нумерации таблицы обработчиков прерываний
/// ([Interrupt descriptor table](https://en.wikipedia.org/wiki/Interrupt_descriptor_table), IDT).
fn generic_apic_interrupt(number: Trap) {
    TRAP_STATS[number].inc();
    LocalApic::end_of_interrupt();
}

/// Обработчик прерывания
/// [таймера APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller#APIC_timer).
extern "x86-interrupt" fn timer(mut context: TrapContext) {
    Process::preempt(&mut context);

    generic_apic_interrupt(Trap::Timer);
}

/// Обработчик ложных прерываний
/// ([spurious interrupt](https://en.wikipedia.org/wiki/Interrupt#Spurious_interrupts))
/// [APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller).
extern "x86-interrupt" fn spurious(_context: TrapContext) {
    generic_apic_interrupt(Trap::Spurious);
}

/// Блокировка, предназначенная для останова всех процессоров кроме одного,
/// в случае возникновения исключения `Trap::DoubleFault`.
static STOP_ALL_CPUS: Spinlock<()> = Spinlock::new(());

#[doc(hidden)]
pub mod test_scaffolding {
    use ku::sync::Spinlock;

    use super::{
        COUNT,
        Idt,
        IdtEntry,
        Trap,
        TrapContext,
    };

    static IDT: Spinlock<Idt> = Spinlock::new(Idt([IdtEntry::missing(); COUNT]));

    pub fn set_debug_handler(handler: extern "x86-interrupt" fn(TrapContext)) {
        let mut idt = IDT.lock();

        *idt = Idt::new();
        idt.0[usize::from(Trap::Debug)].set_handler(handler);
        idt.load();
    }
}
