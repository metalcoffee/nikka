use core::{
    hint,
    mem,
    ptr,
};

use bitflags::bitflags;
use chrono::Duration;
use static_assertions::const_assert_eq;

use ku::time;

use crate::{
    error::{
        Error::InvalidArgument,
        Result,
    },
    log::error,
    memory::{
        BASE_ADDRESS_SPACE,
        Frame,
        FrameGuard,
        KERNEL_MMIO,
        Page,
        Phys,
        Virt,
        size,
    },
    trap::Trap,
};

use register::Register;

/// Идентификатор local APIC и текущего CPU.
pub(crate) type CpuId = u8;

/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
/// для работы с local APIC.
///
/// <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf>, Chapter 10.
/// <https://wiki.osdev.org/APIC#Local_APIC_registers>
#[derive(Clone)]
#[repr(C, align(4096))]
pub(crate) struct LocalApic {
    #[doc(hidden)]
    _0: [Register; 2],

    /// Позволяет узнать идентификатор local APIC и текущего CPU.
    id: Register,

    #[doc(hidden)]
    _1: [Register; 8],

    /// Посылает local APIC сигнал о завершении работы обработчика прерывания ---
    /// [end of interrupt (EOI)](https://en.wikipedia.org/wiki/End_of_interrupt).
    eoi: Register,

    #[doc(hidden)]
    _2: [Register; 3],

    /// Обработчик "фантомных прерываний"
    /// ([Spurious interrupts](https://en.wikipedia.org/wiki/Interrupt#Spurious_interrupts)).
    ///
    /// <https://wiki.osdev.org/APIC#Spurious_Interrupt_Vector_Register>
    spurious: Register,

    #[doc(hidden)]
    _3: [Register; 23],

    /// Регистр ошибок.
    error_status_register: Register,

    #[doc(hidden)]
    _4: [Register; 8],

    /// Позволяет посылать другим процессорам прерывание ---
    /// [inter-processor interrupt](https://en.wikipedia.org/wiki/Inter-processor_interrupt) (IPI).
    /// Эта (нижняя) часть двойного регистра задаёт флаги и дополнительные данные для IPI,
    /// см. [`InterruptCommand`].
    interrupt_command_lo: Register,

    /// Позволяет посылать другим процессорам прерывание ---
    /// [inter-processor interrupt](https://en.wikipedia.org/wiki/Inter-processor_interrupt) (IPI).
    /// Эта (верхняя) часть двойного регистра задаёт целевой процессор для IPI.
    interrupt_command_hi: Register,

    /// Регистр таймера.
    lvt_timer: Register,

    /// Регистр сенсора температуры.
    lvt_termal_sensor: Register,

    /// Регистр переполнения
    /// [hardware performance counter](https://en.wikipedia.org/wiki/Hardware_performance_counter)
    /// (HPC).
    lvt_performance_counter_overflow: Register,

    #[doc(hidden)]
    lvt_lint0: Register,

    #[doc(hidden)]
    lvt_lint1: Register,

    #[doc(hidden)]
    lvt_error: Register,

    /// Начальное значение счётчика таймера.
    timer_initial_count: Register,

    /// Текущее значение счётчика таймера.
    timer_current_count: Register,

    #[doc(hidden)]
    _5: [Register; 4],

    /// Делитель счётчика таймера.
    timer_divide_configuration: Register,

    #[doc(hidden)]
    _6: [Register; 1],
}

const_assert_eq!(mem::size_of::<LocalApic>(), Page::SIZE);

impl LocalApic {
    /// Инициализирует виртуальную страницу для [`LocalApic`] нулями.
    /// Доступна в константном контексте, чтобы можно было инициализировать
    /// с её помощью статическую переменную [`LOCAL_APIC`].
    /// Позже эта страница будет заменена на страницу для доступа к local APIC через интерфейс
    /// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O).
    const fn zero() -> Self {
        Self {
            _0: [Register::zero(); 2],
            id: Register::zero(),
            _1: [Register::zero(); 8],
            eoi: Register::zero(),
            _2: [Register::zero(); 3],
            spurious: Register::zero(),
            _3: [Register::zero(); 23],
            error_status_register: Register::zero(),
            _4: [Register::zero(); 8],
            interrupt_command_lo: Register::zero(),
            interrupt_command_hi: Register::zero(),
            lvt_timer: Register::zero(),
            lvt_termal_sensor: Register::zero(),
            lvt_performance_counter_overflow: Register::zero(),
            lvt_lint0: Register::zero(),
            lvt_lint1: Register::zero(),
            lvt_error: Register::zero(),
            timer_initial_count: Register::zero(),
            timer_current_count: Register::zero(),
            _5: [Register::zero(); 4],
            timer_divide_configuration: Register::zero(),
            _6: [Register::zero(); 1],
        }
    }

    /// Инициализирует local APIC, в том числе включает прерывание таймера.
    pub(super) fn init() {
        /// Количество тиков процессора между прерываниями от APIC таймера.
        const TSCS_PER_INTERRUPT: u32 = 100_000_000;

        let local_apic = Self::get();

        local_apic.enable();
        local_apic.disable_lvts();
        local_apic.init_timer(TSCS_PER_INTERRUPT);
        Self::end_of_interrupt();
    }

    /// Инициализирует [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
    /// для работы с local APIC по физическому адресу `address`.
    pub(super) fn map(address: Phys) -> Result<()> {
        let local_apic = Self::get();
        let virt = Virt::from_ref(local_apic);
        let page = Page::new(virt)?;
        let frame = Frame::new(address)?;
        let flags = KERNEL_MMIO;
        unsafe {
            BASE_ADDRESS_SPACE.lock().map_page_to_frame(page, frame, flags)?;
        }
        
        Ok(())
    }

    /// Посылает local APIC сигнал о завершении работы обработчика прерывания ---
    /// [end of interrupt (EOI)](https://en.wikipedia.org/wiki/End_of_interrupt).
    ///
    /// <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf>,
    /// Chapter 10.8.5 "Signaling Interrupt Servicing Completion"
    ///
    /// <https://wiki.osdev.org/APIC#EOI_Register>
    pub(crate) fn end_of_interrupt() {
        Self::get().eoi.set(0);
    }

    /// Позволяет узнать идентификатор local APIC и текущего CPU.
    ///
    /// <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf>,
    /// Chapter 10.4.6 "Local APIC ID"
    pub(crate) fn id() -> CpuId {
        (Self::get().id.get() >> Self::ID_SHIFT).try_into().unwrap()
    }

    /// Посылает прерывание на Application Processor с идентификатором `id`,
    /// предназначенное для его инициализации.
    /// Физический адрес процедуры инициализации задаёт `boot_address`.
    ///
    /// MultiProcessor Specification 4.1 part B.4 "Application Processor Startup"
    pub(super) fn send_init(
        id: CpuId,
        boot_address: Phys,
    ) -> Result<()> {
        /// Максимальный номер физического фрейма, который может задавать адрес
        /// кода загрузки процессора.
        const MAX_BOOT_FRAME: u32 = 0xFF;

        let boot_page = size::try_into::<u32>(Frame::new(boot_address)?.index())?;
        if boot_page > MAX_BOOT_FRAME {
            return Err(InvalidArgument);
        }

        let local_apic = Self::get();

        let init_data = InterruptCommand::INIT | InterruptCommand::TRIGGER_MODE_LEVEL;

        local_apic.send_ipi(id, (init_data | InterruptCommand::LEVEL_ASSERT).bits());
        time::delay(Duration::microseconds(200));

        local_apic.send_ipi(id, (init_data | InterruptCommand::LEVEL_DEASSERT).bits());
        time::delay(Duration::microseconds(200));

        for _ in 0 .. 2 {
            local_apic.send_ipi(id, InterruptCommand::START_UP.bits() | boot_page);
            time::delay(Duration::microseconds(200));
        }

        Ok(())
    }

    /// Посылает процессору `id` прерывание
    /// ([inter-processor interrupt](https://en.wikipedia.org/wiki/Inter-processor_interrupt), IPI)
    /// с дополнительными данными `data`.
    ///
    /// <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf>,
    /// 10.6 "Issuing Interprocessor Interrupts"
    fn send_ipi(
        &mut self,
        id: CpuId,
        data: u32,
    ) {
        self.interrupt_command_hi.set(u32::from(id) << Self::ID_SHIFT);
        self.interrupt_command_lo.set(data);

        while (self.interrupt_command_lo.get() & InterruptCommand::SEND_PENDING.bits()) != 0 {
            hint::spin_loop();
        }

        let error = self.error_status_register.get();
        if error != 0 {
            error!(cpu = id, data, error, "error sending IPI");
        }
    }

    /// Возвращает структуру [`LocalApic`] текущего CPU.
    fn get() -> &'static mut Self {
        unsafe { &mut *ptr::addr_of_mut!(LOCAL_APIC) }
    }

    /// Разрешает использование local APIC.
    ///
    /// <https://wiki.osdev.org/APIC#Spurious_Interrupt_Vector_Register>
    fn enable(&mut self) {
        /// Включает использование local APIC.
        const ENABLE_LOCAL_APIC: u32 = 1 << 8;
        self.spurious
            .set(ENABLE_LOCAL_APIC | size::try_into::<u32>(Trap::Spurious.into()).unwrap());
    }

    /// Запрещает ненужные нам локальные прерывания.
    ///
    /// <https://wiki.osdev.org/APIC#Local_Vector_Table_Registers>
    fn disable_lvts(&mut self) {
        /// Отключает получение прерывания.
        const MASK_INTERRUPT: u32 = 1 << 16;

        for interrupt in [
            &mut self.lvt_termal_sensor,
            &mut self.lvt_performance_counter_overflow,
            &mut self.lvt_lint1,
            &mut self.lvt_error,
        ] {
            interrupt.update(|x| x | MASK_INTERRUPT);
        }
    }

    /// Инициализирует таймер local APIC в периодическом режиме с делителем 1
    /// и прерыванием номер [`Trap::Timer`].
    ///
    /// <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf>,
    /// Chapter 10.5.4
    fn init_timer(
        &mut self,
        tscs_per_interrupt: u32,
    ) {
        /// Задаёт делитель таймера равный 1.
        const DIVIDE_BY_1: u32 = 0b1011;

        self.timer_divide_configuration.set(DIVIDE_BY_1);
        self.timer_initial_count.set(tscs_per_interrupt);

        /// Задаёт периодический режим таймера.
        const PERIODIC_MODE: u32 = 0b01 << 17;

        self.lvt_timer
            .set(PERIODIC_MODE | size::try_into::<u32>(Trap::Timer.into()).unwrap());
    }

    /// Сдвиг для [`CpuId`] внутри [`LocalApic::id`].
    const ID_SHIFT: usize = 24;
}

/// Задаёт формат одного регистра local APIC.
mod register {
    /// Задаёт формат одного регистра local APIC.
    #[derive(Clone, Copy)]
    #[repr(C)]
    pub(super) struct Register(u32, #[doc(hidden)] [u32; 3]);

    impl Register {
        /// Возвращает регистр, инициализированный нулём.
        /// Доступна в константном контексте.
        pub(super) const fn zero() -> Self {
            Self(0, [0; 3])
        }

        /// Читает значение из регистра.
        pub(super) fn get(&self) -> u32 {
            unsafe { (&self.0 as *const u32).read_volatile() }
        }

        /// Записывает значение в регистр.
        pub(super) fn set(
            &mut self,
            value: u32,
        ) {
            unsafe { (&mut self.0 as *mut u32).write_volatile(value) }
        }

        /// Обновляет значение в регистре с помощью заданной `f`.
        pub(super) fn update<F: FnOnce(u32) -> u32>(
            &mut self,
            f: F,
        ) {
            self.set(f(self.get()));
        }
    }
}

bitflags! {
    /// Поля регистра данных [`LocalApic::interrupt_command_lo`] для
    /// [inter-processor interrupt](https://en.wikipedia.org/wiki/Inter-processor_interrupt) (IPI).
    ///
    /// <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf>,
    /// 10.6.1 Interrupt Command Register (ICR)
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct InterruptCommand: u32 {
        /// Выполнить инициализацию процессора.
        const INIT = 0b101 << 8;

        /// Выполнить код загрузки по заданному адресу.
        const START_UP = 0b110 << 8;

        /// Признак того, что последнее
        /// [IPI](https://en.wikipedia.org/wiki/Inter-processor_interrupt)
        /// пока не доставлено целевому процессору.
        const SEND_PENDING = 1 << 12;

        /// Высокий уровень сигнала для прерывания [`InterruptCommand::INIT`].
        const LEVEL_ASSERT = 1 << 14;

        /// Низкий уровень сигнала для прерывания [`InterruptCommand::INIT`].
        const LEVEL_DEASSERT = 0 << 14;

        /// Выбрать режим "уровень" для прерывания [`InterruptCommand::INIT`].
        const TRIGGER_MODE_LEVEL = 1 << 15;
    }
}

/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
/// для работы с local APIC.
static mut LOCAL_APIC: LocalApic = LocalApic::zero();

#[doc(hidden)]
pub mod test_scaffolding {
    use ku::memory::Virt;

    use super::LocalApic;

    pub fn local_apic() -> Virt {
        Virt::from_ref(LocalApic::get())
    }

    pub fn id() -> u8 {
        LocalApic::id()
    }
}
