use core::{
    arch::{
        asm,
        naked_asm,
    },
    fmt,
    mem,
};

use x86::bits64::registers;
use x86_64::structures::gdt::SegmentSelector;

use ku::process::{
    MiniContext,
    Pid,
    RFlags,
    ResultCode,
};

use crate::{
    error::Result,
    gdt::Gdt,
    memory::Virt,
    smp::KERNEL_RSP_OFFSET_IN_CPU,
};

// Used in docs.
#[allow(unused)]
use {
    crate::process,
    ku::ProcessInfo,
};

/// Состояние регистров пользовательского процесса.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct Registers {
    /// Содержимое регистра `RAX` контекста кода пользователя.
    rax: usize,

    /// Содержимое регистров `RBX`, `RCX` и `RDX` контекста кода пользователя.
    gpr1: [usize; 3],

    /// Содержимое регистра `RDI` контекста кода пользователя.
    rdi: usize,

    /// Содержимое регистра `RSI` контекста кода пользователя.
    rsi: usize,

    /// Содержимое регистров `RBP`, `R8`--`R15` контекста кода пользователя.
    gpr2: [usize; 9],

    /// Контекст исполнения, позволяющий задать уровень привилегий --- ядра или пользователя.
    user_context: ModeContext,
}

impl Registers {
    /// Создаёт регистры процесса с заданным начальным минимальным `context` и передаёт ему
    /// аргументы `rax`, `rdi` и `rsi` в соответствующих регистрах:
    ///   - Регистр `rax` содержит [`ResultCode::Ok`].
    ///   - Регистр `rdi` содержит [`Pid::Current`].
    ///   - Регистр `rsi` содержит адрес [`ProcessInfo`] процесса.
    ///
    /// Остальные регистры заполняет нулями.
    ///
    /// Значения в `rax` и `rdi` соответствуют результату,
    /// который возвращается потомку в случае успешного `syscall::exofork()`.
    /// Значение в `rsi` --- информация для процесса, созданного с нуля через
    /// [`process::create_process()`].
    /// Для единообразия, эти регистры имеют такие значения при любом варианте запуска процесса,
    /// даже если они не будут им использованы.
    pub(super) fn new(
        context: MiniContext,
        process_info: Virt,
    ) -> Self {
        Self {
            rax: ResultCode::Ok.into(),
            gpr1: [0; 3],
            rdi: Pid::Current.into_usize(),
            rsi: process_info.into_usize(),
            gpr2: [0; 9],
            user_context: ModeContext::user_context(context),
        }
    }

    /// Дублирует регистры, заменяя в копии значения `rax`, `rdi` и `rsi`.
    pub(super) fn duplicate(
        &self,
        rax: usize,
        rdi: usize,
        rsi: usize,
    ) -> Self {
        Self {
            rax,
            gpr1: self.gpr1,
            rdi,
            rsi,
            gpr2: self.gpr2,
            user_context: self.user_context,
        }
    }

    /// Возвращает минимальный контекст процесса.
    pub(super) fn mini_context(&self) -> MiniContext {
        self.user_context.mini_context()
    }

    /// Устанавливает минимальный контекст процесса.
    pub(super) fn set_mini_context(
        &mut self,
        context: MiniContext,
    ) {
        self.user_context.set_mini_context(context);
    }

    /// Устанавливает расширенный контекст процесса с уровнем привилегий и регистром флагов.
    pub(super) fn set_mode_context(
        &mut self,
        context: ModeContext,
    ) {
        self.user_context = context;
    }

    /// Сохраняет значение в регистр `rax`.
    pub(super) fn set_rax(
        &mut self,
        rax: usize,
    ) {
        self.rax = rax;
    }

    /// Сохраняет значение в регистр `rdi`.
    pub(super) fn set_rdi(
        &mut self,
        rdi: usize,
    ) {
        self.rdi = rdi;
    }

    /// Переключается в процесс, заданный регистрами `Registers`.
    ///
    /// Сохраняет контекст ядра и переключается в контекст пользователя.
    /// При этом текущий уровень привилегий меняется с уровня ядра на уровень пользователя.
    /// После возвращения из режима пользователя сохраняет контекст пользователя и
    /// восстанавливает ранее сохранённый контекст ядра.
    #[allow(named_asm_labels)]
    #[inline(never)] // For the named label to be a unique link symbol.
    pub(super) unsafe fn switch_to(registers: *mut Registers) {
        let old_kernel_rsp = rsp();

        const USER_REGISTERS_SIZE: usize = mem::size_of::<Registers>() - mem::size_of::<ModeContext>();

        unsafe {
            asm!(
                "
                push rbx
                push rbp

                push {registers}

                mov gs:[{rsp_offset}], rsp

                cli

                mov rsp, {registers}

                pop rax
                pop rbx
                pop rcx
                pop rdx
                pop rdi
                pop rsi
                pop rbp
                pop r8
                pop r9
                pop r10
                pop r11
                pop r12
                pop r13
                pop r14
                pop r15

                iretq

            store_user_mode_context:

                mov rsp, gs:[{rsp_offset}]
                xchg rax, [rsp]
                lea rsp, [rax + {user_registers_size}]

                push r15
                push r14
                push r13
                push r12
                push r11
                push r10
                push r9
                push r8
                push rbp
                push rsi
                push rdi
                push rdx
                push rcx
                push rbx
                
                mov rax, gs:[{rsp_offset}]
                push qword ptr [rax]

                mov rsp, gs:[{rsp_offset}]
                
                pop rax
                push rax
                
                sti

            switch_to_kernel_mode:


                add rsp, 8


                pop rbp
                pop rbx
                ",

                registers = in(reg) registers,
                rsp_offset = const KERNEL_RSP_OFFSET_IN_CPU,
                user_registers_size = const USER_REGISTERS_SIZE,
                lateout("rax") _,
                lateout("rcx") _,
                lateout("rdx") _,
                lateout("rdi") _,
                lateout("rsi") _,
                lateout("r8") _,
                lateout("r9") _,
                lateout("r10") _,
                lateout("r11") _,
                lateout("r12") _,
                lateout("r13") _,
                lateout("r14") _,
                lateout("r15") _,
            );
        }

        let new_kernel_rsp = rsp();

        assert!(
            old_kernel_rsp.is_ok() && new_kernel_rsp.is_ok(),
            "check that the kernel RSP is saved and restored correctly",
        );

        assert_eq!(
            old_kernel_rsp, new_kernel_rsp,
            concat!(
                "check that the kernel RSP is saved and restored correctly and the code ",
                "pushes to the stack the same amount of information as it pops from it",
            ),
        );
    }

    /// Вытесняет текущий исполняющийся процесс с процессора по его собственному запросу.
    ///
    /// Не сохраняет контекст процесса пользователя,
    /// он должен сохранить его сам в процедуре системного вызова `syscall::sched_yield()`.
    /// Возвращается в контекст ядра, из которого этот процесс был запущен.
    /// Текущий контекст ядра уничтожается.
    #[unsafe(naked)]
    pub(super) unsafe extern "C" fn sched_yield() -> ! {
        naked_asm!("jmp switch_to_kernel_mode");
    }

    /// Вытесняет текущий исполняющийся процесс с процессора принудительно.
    ///
    /// Сохраняет контекст процесса пользователя,
    /// так как он не готов к вытеснению и порче регистров.
    /// Возвращается в контекст ядра, из которого этот процесс был запущен.
    /// Текущий контекст ядра уничтожается.
    ///
    /// Реализация прыгает внутрь метода [`Registers::switch_to()`] на метку,
    /// с которой начинается сохранение контекста пользователя и
    /// восстановление контекста ядра.
    #[unsafe(naked)]
    pub(super) unsafe extern "C" fn switch_from() -> ! {
        naked_asm!("jmp store_user_mode_context");
    }
}

impl fmt::Display for Registers {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ rax: {:#X}, rdi: {:#X}, rsi: {:#X}, {} }}",
            self.rax, self.rdi, self.rsi, self.user_context,
        )
    }
}

/// Контекст исполнения, позволяющий задать уровень привилегий --- ядра или пользователя.
///
/// Имеет в памяти ровно такое представление, какого требует инструкция
/// [iret](https://www.felixcloutier.com/x86/iret:iretd:iretq).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ModeContext {
    /// [Instruction Pointer](https://wiki.osdev.org/CPU_Registers_x86-64#Pointer_Registers)
    rip: Virt,

    /// [Code Segment](https://wiki.osdev.org/CPU_Registers_x86-64#Segment_Registers)
    cs: usize,

    /// [RFLAGS Register](https://wiki.osdev.org/CPU_Registers_x86-64#RFLAGS_Register)
    rflags: RFlags,

    /// [Stack Pointer](https://wiki.osdev.org/CPU_Registers_x86-64#General_Purpose_Registers)
    rsp: Virt,

    /// [Stack Segment](https://wiki.osdev.org/CPU_Registers_x86-64#Segment_Registers)
    ss: usize,
}

impl ModeContext {
    /// Собирает [`ModeContext`] из его частей.
    fn new(
        code: SegmentSelector,
        data: SegmentSelector,
        context: MiniContext,
        rflags: RFlags,
    ) -> Self {
        Self {
            rip: context.rip(),
            cs: code.0.into(),
            rflags,
            rsp: context.rsp(),
            ss: data.0.into(),
        }
    }

    /// Возвращает [`ModeContext`] для заданного `context` с привилегиями пользователя.
    fn user_context(context: MiniContext) -> Self {
        Self::new(
            Gdt::user_code(),
            Gdt::user_data(),
            context,
            RFlags::INTERRUPT_FLAG,
        )
    }

    /// Возвращает [`ModeContext`] для заданного `rsp` с привилегиями ядра.
    /// В качестве `rip` использует функцию [`Registers::switch_from()`],
    /// то есть при переходе в этот контекст будет запущена именно она.
    pub(crate) fn kernel_context(rsp: Virt) -> Self {
        Self::new(
            Gdt::kernel_code(),
            Gdt::kernel_data(),
            MiniContext::new(Virt::from_ptr(Registers::switch_from as *const ()), rsp),
            RFlags::default(),
        )
    }

    /// Возвращает [`MiniContext`].
    pub fn mini_context(&self) -> MiniContext {
        MiniContext::new(self.rip, self.rsp)
    }

    /// Устанавливает [`MiniContext`].
    pub fn set_mini_context(
        &mut self,
        context: MiniContext,
    ) {
        self.rip = context.rip();
        self.rsp = context.rsp();
    }

    /// Возвращает `true`, если контекст имеет привилегии пользователя.
    pub fn is_user_mode(&self) -> bool {
        assert_eq!(
            Self::is_user_mode_segment(self.cs),
            Self::is_user_mode_segment(self.ss),
        );

        Self::is_user_mode_segment(self.cs)
    }

    /// Возвращает `true`, если селектор сегмента `segment_selector` имеет привилегии пользователя.
    fn is_user_mode_segment(segment_selector: usize) -> bool {
        segment_selector & Self::CPL_MASK != Self::RING_0
    }

    /// Возвращает текстовое имя режима контекста --- `"kernel"` или `"user"`.
    fn mode(&self) -> &'static str {
        if self.is_user_mode() {
            "user"
        } else {
            "kernel"
        }
    }

    /// Маска уровня привилегий ([кольца защиты](https://en.wikipedia.org/wiki/Protection_ring))
    /// записанного в селекторе сегмента.
    ///
    /// (Current Privilege Level, если селектор записан в регистр сегмента кода CS.)
    const CPL_MASK: usize = 0x3;

    /// Уровень привилегий ([кольцо защиты](https://en.wikipedia.org/wiki/Protection_ring)) ядра.
    const RING_0: usize = 0x0;
}

impl fmt::Display for ModeContext {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ mode: {}, cs:rip: {:#06X}:{}, ss:rsp: {:#06X}:{}, rflags: {} }}",
            self.mode(),
            self.cs,
            self.rip,
            self.ss,
            self.rsp,
            self.rflags,
        )
    }
}

/// Возвращает текущее значение регистра `RSP`.
fn rsp() -> Result<Virt> {
    Virt::new_u64(registers::rsp())
}

#[doc(hidden)]
pub(super) mod test_scaffolding {
    use super::{
        RFlags,
        Registers,
    };

    pub(in super::super) fn disable_interrupts(registers: &mut Registers) {
        registers.user_context.rflags &= !RFlags::INTERRUPT_FLAG;
    }

    pub(in super::super) fn registers(registers: &Registers) -> [usize; 15] {
        let mut result = [0; 15];
        result[0] = registers.rax;
        result[1 .. 4].copy_from_slice(&registers.gpr1);
        result[4] = registers.rdi;
        result[5] = registers.rsi;
        result[6 .. 15].copy_from_slice(&registers.gpr2);
        result
    }
}
