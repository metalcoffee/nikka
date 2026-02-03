use core::{
    fmt,
    mem,
};

use enum_iterator::Sequence;
use memoffset::offset_of;
use num_enum::{
    IntoPrimitive,
    TryFromPrimitive,
};
use x86_64::registers::control::Cr2;

use crate::{
    error::Result,
    memory::{
        PageFaultInfo,
        Virt,
    },
};

use super::{
    MiniContext,
    mini_context::RSP_OFFSET_IN_MINI_CONTEXT,
};

/// Исключение или прерывание.
#[derive(
    Clone, Copy, Debug, Eq, IntoPrimitive, Ord, PartialEq, PartialOrd, Sequence, TryFromPrimitive,
)]
#[repr(usize)]
pub enum Trap {
    /// [Exceptions: Division Error](https://wiki.osdev.org/Exception#Division_Error)
    DivideError = 0x00,

    /// [Exceptions: Debug](https://wiki.osdev.org/Exception#Debug)
    Debug = 0x01,

    /// [Non-maskable interrupt](https://en.wikipedia.org/wiki/Non-maskable_interrupt),
    /// [Exceptions: Non-maskable interrupt](https://wiki.osdev.org/Non_Maskable_Interrupt)
    NonMaskableInterrupt = 0x02,

    /// [Exceptions: Breakpoint](https://wiki.osdev.org/Exception#Breakpoint)
    Breakpoint = 0x03,

    /// [Exceptions: Overflow](https://wiki.osdev.org/Exception#Overflow)
    Overflow = 0x04,

    /// [Exceptions: Bound Range Exceeded](https://wiki.osdev.org/Exception#Bound_Range_Exceeded)
    BoundRangeExceeded = 0x05,

    /// [Exceptions: Invalid Opcode](https://wiki.osdev.org/Exception#Invalid_Opcode)
    InvalidOpcode = 0x06,

    /// [Exceptions: Device Not Available](https://wiki.osdev.org/Exception#Device_Not_Available)
    DeviceNotAvailable = 0x07,

    /// [Double Fault](https://en.wikipedia.org/wiki/Double_fault),
    /// [Exceptions: Double Fault](https://wiki.osdev.org/Exception#Double_Fault)
    DoubleFault = 0x08,

    /// [Exceptions: Invalid TSS](https://wiki.osdev.org/Exception#Invalid_TSS)
    InvalidTss = 0x0A,

    /// [Exceptions: Segment Not Present](https://wiki.osdev.org/Exception#Segment_Not_Present)
    SegmentNotPresent = 0x0B,

    /// [Exceptions: Stack-Segment Fault](https://wiki.osdev.org/Exception#Stack-Segment_Fault)
    StackSegmentFault = 0x0C,

    /// [General Protection Fault](https://en.wikipedia.org/wiki/General_protection_fault),
    /// [Exceptions: General Protection Fault](https://wiki.osdev.org/Exception#General_Protection_Fault)
    GeneralProtectionFault = 0x0D,

    /// [Page Fault](https://en.wikipedia.org/wiki/Page_fault),
    /// [Exceptions: Page Fault](https://wiki.osdev.org/Exception#Page_Fault)
    PageFault = 0x0E,

    /// [Exceptions: x87 Floating-Point Exception](https://wiki.osdev.org/Exception#x87_Floating-Point_Exception)
    X87FloatingPoint = 0x10,

    /// [Exceptions: Alignment Check](https://wiki.osdev.org/Exception#Alignment_Check)
    AlignmentCheck = 0x11,

    /// [Machine-check exception](https://en.wikipedia.org/wiki/Machine-check_exception),
    /// [Exceptions: Machine Check](https://wiki.osdev.org/Exception#Machine_Check)
    MachineCheck = 0x12,

    /// [Exceptions: SIMD Floating-Point Exception](https://wiki.osdev.org/Exception#SIMD_Floating-Point_Exception)
    SimdFloatingPoint = 0x13,

    /// [Exceptions: Security Exception](https://wiki.osdev.org/Exception#Security_Exception)
    Virtualization = 0x14,

    /// [Exceptions: Security Exception](https://wiki.osdev.org/Exception#Security_Exception)
    SecurityException = 0x1E,

    /// Номер прерывания таймера [Intel 8253/8254](https://en.wikipedia.org/wiki/Intel_8253)
    /// ([programmable interval timer, PIT](https://en.wikipedia.org/wiki/Programmable_interval_timer)).
    Pit = 0x20,

    /// Номер прерывания клавиатуры.
    Keyboard,

    /// Номер входа первого контроллера
    /// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259),
    /// к которому каскадно подключён второй такой же.
    Cascade,

    /// Номер прерывания
    /// [последовательных портов](https://en.wikipedia.org/wiki/Serial_port) номер 2 и 4.
    Com2,

    /// Номер прерывания
    /// [последовательных портов](https://en.wikipedia.org/wiki/Serial_port) номер 1 и 3.
    Com1,

    /// [Номер прерывания](https://en.wikipedia.org/wiki/Parallel_port#Port_addresses)
    /// второго параллельного порта
    /// ([Parallel port](https://en.wikipedia.org/wiki/Parallel_port)).
    /// Так как через параллельные порты чаще всего подключались принтеры
    /// ([Line printer](https://en.wikipedia.org/wiki/Line_printer)),
    /// сохранилось их сокращение LPT.
    Lpt2,

    /// Номер прерывания контроллера [дискет](https://en.wikipedia.org/wiki/Floppy_disk).
    FloppyDisk,

    /// [Номер прерывания](https://en.wikipedia.org/wiki/Parallel_port#Port_addresses)
    /// первого и третьего параллельного порта
    /// ([Parallel port](https://en.wikipedia.org/wiki/Parallel_port)).
    /// Так как через параллельные порты чаще всего подключались принтеры
    /// ([Line printer](https://en.wikipedia.org/wiki/Line_printer)),
    /// сохранилось их сокращение LPT.
    Lpt1,

    /// Номер обработчика прерываний
    /// [часов реального времени (Real-time clock, RTC)](https://en.wikipedia.org/wiki/Real-time_clock).
    Rtc,

    /// Номер прерывания входа `0x9` каскадной пары
    /// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
    Free29,

    /// Номер прерывания входа `0xA` каскадной пары
    /// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
    Free2A,

    /// Номер прерывания входа `0xB` каскадной пары
    /// [PIC 8259](https://en.wikipedia.org/wiki/Intel_8259).
    Free2B,

    /// Номер прерывания мыши.
    Ps2Mouse,

    /// Номер прерывания сопроцессора.
    Coprocessor,

    /// Номер прерывания первого контроллера
    /// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA).
    Ata0,

    /// Номер прерывания второго контроллера
    /// [PATA](https://en.wikipedia.org/wiki/Parallel_ATA).
    Ata1,

    /// Номер прерывания
    /// [таймера APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller#APIC_timer).
    Timer,

    /// Номер ложных прерываний
    /// ([spurious interrupt](https://en.wikipedia.org/wiki/Interrupt#Spurious_interrupts))
    /// [APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller).
    Spurious,
}

// ANCHOR: trap_info
/// Информация об исключении процессора.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TrapInfo {
    /// Номер исключения.
    number: usize,

    /// Информация об исключении, предоставляемая процессором.
    info: Info,

    /// Контекст, в котором возникло исключение.
    context: MiniContext,

    /// [`TrapInfo`] может быть сохранено в тот же стек,
    /// на который указывает [`TrapInfo::context`].
    /// Это происходит при рекурсивном исключении ---
    /// то есть когда исключение происходит внутри обработчика исключений.
    /// В этом случае функции `lib::syscall::trap_trampoline` и
    /// `lib::syscall::trap_handler_invoker` сохранят адрес возврата на тот же стек
    /// [`TrapInfo::context`].
    /// Что приведёт к перезаписи содержимого [`TrapInfo`].
    /// Поле [`TrapInfo::return_address_placeholder`] служит для избежания перезаписи
    /// существенных полей --- оно имеет тот же размер и расположение, как и адрес возврата.
    return_address_placeholder: [u8; Self::PLACEHOLDER_SIZE],
}
// ANCHOR_END: trap_info

impl TrapInfo {
    /// Размер адреса возврата из функции, см. [`TrapInfo::return_address_placeholder`].
    const PLACEHOLDER_SIZE: usize = mem::size_of::<Virt>();

    /// Создаёт информацию об исключении.
    ///
    /// - number --- номер исключения,
    /// - info --- информация об исключении, предоставляемая процессором.
    /// - context --- контекст, в котором возникло исключение.
    pub fn new(
        number: usize,
        info: Info,
        context: MiniContext,
    ) -> Self {
        Self {
            number,
            info,
            context,
            return_address_placeholder: [0; Self::PLACEHOLDER_SIZE],
        }
    }

    /// Номер исключения.
    pub fn number(&self) -> usize {
        self.number
    }

    /// Информация об исключении, предоставляемая процессором.
    pub fn info(&self) -> Info {
        self.info
    }

    /// Контекст, в котором возникло исключение.
    pub fn context(&self) -> MiniContext {
        self.context
    }

    /// Записывает на стек контекста исключения адрес возникновения исключения.
    /// После этого, если переключить стек в изменившийся [`TrapInfo::context`],
    /// и выполнить инструкцию `ret`,
    /// то регистры `rip` и `rsp` окажутся в состоянии, равном исходному [`TrapInfo::context`].
    ///
    /// # Safety
    ///
    /// - Контекст исключения, сохранённый в [`TrapInfo::context`] должен быть корректен.
    /// - В стеке контекста исключения должно быть достаточно места для адреса возврата ---
    ///   [`Virt`].
    pub unsafe fn prepare_for_ret(&mut self) -> Result<()> {
        unsafe {
            *self.context.push::<Virt>()?.try_into_mut()? = self.context.rip();
        }
        Ok(())
    }
}

impl fmt::Display for TrapInfo {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ #{}, {}, {} }}",
            self.number, self.info, self.context
        )
    }
}

/// Информация об исключении, предоставляемая процессором.
#[derive(Clone, Copy, Debug)]
pub enum Info {
    /// Исключение не имеет дополнительной информации.
    None,

    /// Код ошибки, сохраняемый процессором на стеке для некоторых исключений.
    Code(usize),

    /// Информация об исключении обращения к странице виртуальной памяти.
    PageFault {
        /// Адрес к которому происходило обращение.
        address: Virt,

        /// Причина некорректности обращения.
        code: PageFaultInfo,
    },
}

impl Info {
    /// Информация об исключении `trap`, предоставляемая процессором в виде кода `error_code`.
    pub fn new(
        trap: Trap,
        error_code: usize,
    ) -> Self {
        match trap {
            Trap::AlignmentCheck |
            Trap::DoubleFault |
            Trap::GeneralProtectionFault |
            Trap::InvalidTss |
            Trap::SecurityException |
            Trap::SegmentNotPresent |
            Trap::StackSegmentFault => Info::Code(error_code),

            Trap::PageFault => Info::PageFault {
                address: Cr2::read().expect("read bad value from %cr2").into(),
                code: PageFaultInfo::from_bits_truncate(error_code),
            },

            _ => Info::None,
        }
    }
}

impl fmt::Display for Info {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        match self {
            Info::None => write!(formatter, "{{ }}"),
            Info::Code(code) => write!(formatter, "{{ code: {code} }}"),
            Info::PageFault { address, code } => {
                write!(formatter, "{{ address: {address}, code: {code} }}")
            },
        }
    }
}

/// Смещение поля для регистра `rsp` контекста исключения в структуре [`TrapInfo`].
/// Позволяет обращаться к этому полю из ассемблерных вставок.
pub const RSP_OFFSET_IN_TRAP_INFO: usize =
    offset_of!(TrapInfo, context) + RSP_OFFSET_IN_MINI_CONTEXT;
