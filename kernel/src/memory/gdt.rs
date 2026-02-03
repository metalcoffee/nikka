use core::{
    fmt,
    mem,
};

use lazy_static::lazy_static;
use x86_64::{
    PrivilegeLevel,
    instructions::{
        segmentation::{
            CS,
            DS,
            ES,
            SS,
            Segment,
        },
        tables,
    },
    structures::{
        DescriptorTablePointer,
        gdt::{
            DescriptorFlags,
            SegmentSelector,
        },
        tss::TaskStateSegment,
    },
};

use ku::sync::spinlock::Spinlock;

use crate::{
    error::{
        Error::InvalidArgument,
        Result,
    },
    log::info,
    smp::CpuId,
};

use super::{
    Phys,
    Virt,
    size,
    tss::TSS,
};

// Used in docs.
#[allow(unused)]
use crate as kernel;

/// Инициализирует глобальную таблицу дескрипторов
/// ([Global Descriptor Table](https://en.wikipedia.org/wiki/Global_Descriptor_Table), GDT).
pub(crate) fn init() {
    GDT.lock().load();

    unsafe {
        tables::load_tss(Gdt::tss(0));
    }

    info!("GDT init");
}

/// Описатель размера и положения таблицы дескрипторов для 32--битного режима.
/// В некоторых документах называется `pseudo-descriptor`.
/// Передаётся в команду процессора [`lgdt`](https://www.felixcloutier.com/x86/lgdt:lidt).
#[derive(Copy, Clone, Default, Eq, PartialEq)]
#[repr(C, packed)]
pub(crate) struct RealModePseudoDescriptor {
    /// Максимальное смещение внутри таблицы дескрипторов в байтах.
    /// То есть, её размер в байтах минус 1.
    limit: u16,

    /// 32--битный физический адрес таблицы дескрипторов.
    base: u32,
}

impl RealModePseudoDescriptor {
    /// Создаёт описатель размера и положения таблицы дескрипторов для 32--битного режима.
    /// Размер таблицы вычисляется по её типу `T`, а адрес задаётся аргументом `address`.
    fn new<T>(address: Phys) -> Result<Self> {
        /// Процессор 80286 имеет ограничение для физических адресов в 16 MiB.
        const MAX_REAL_MODE_BASE: u32 = 16 << 20;

        let base = address.try_into()?;

        if base < MAX_REAL_MODE_BASE {
            Ok(Self {
                limit: (mem::size_of::<T>() - 1).try_into()?,
                base,
            })
        } else {
            Err(InvalidArgument)
        }
    }
}

impl fmt::Debug for RealModePseudoDescriptor {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ base: {}, limit: 0x{:04X} }}",
            { Phys::new_u32(self.base) },
            { self.limit }
        )
    }
}

/// Глобальная таблица дескрипторов
/// ([Global Descriptor Table](https://en.wikipedia.org/wiki/Global_Descriptor_Table), GDT)
/// с поддержкой
/// симметричной многопроцессорности
/// ([Symmetric multiprocessing](https://en.wikipedia.org/wiki/Symmetric_multiprocessing), SMP).
/// Каждому процессору нужен отдельный сегмент состояния задачи
/// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment), TSS).
/// Параметр `TSS_COUNT` задаёт их максимальное количество.
#[repr(C, packed)]
pub(crate) struct SmpGdt<const TSS_COUNT: usize> {
    /// Основной набор сегментов: нулевой и четыре сегмента для кода/данных ядра/пользователя.
    basic: [DescriptorFlags; BASIC_COUNT],

    /// Массив дескрипторов сегментов состояния задачи
    /// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment), TSS).
    tss: [TssEntry; TSS_COUNT],
}

impl<const TSS_COUNT: usize> SmpGdt<TSS_COUNT> {
    /// Создаёт глобальную таблицу дескрипторов.
    pub(crate) fn new() -> Self {
        let mut gdt = Self {
            basic: [DescriptorFlags::empty(); BASIC_COUNT],
            tss: [TssEntry::default(); TSS_COUNT],
        };

        for descriptor in [
            DescriptorFlags::KERNEL_CODE64,
            DescriptorFlags::KERNEL_DATA,
            DescriptorFlags::USER_CODE64,
            DescriptorFlags::USER_DATA,
        ] {
            gdt.basic[usize::from(Self::basic_selector(descriptor).index())] = descriptor;
        }

        gdt
    }

    /// Создаёт описатель размера и положения таблицы дескрипторов для 32--битного режима.
    /// Размер таблицы вычисляется по её типу `Self`, а адрес задаётся аргументом `address`.
    pub(crate) fn real_mode_pseudo_descriptor(address: Phys) -> Result<RealModePseudoDescriptor> {
        RealModePseudoDescriptor::new::<Self>(address)
    }

    /// Записывает в GDT дескриптор сегмента состояния задачи `tss`
    /// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment))
    /// для процессора номер `cpu`.
    pub(crate) fn set_tss(
        &mut self,
        cpu: CpuId,
        tss: &TaskStateSegment,
    ) {
        let cpu = usize::from(cpu);
        assert!(cpu < TSS_COUNT);

        self.tss[cpu] = TssEntry::new(tss);
    }

    /// Возвращает селектор сегмента состояния задачи `tss`
    /// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment))
    /// процессора номер `cpu`.
    pub(crate) fn tss(cpu: CpuId) -> SegmentSelector {
        /// Количество записей типа [`DescriptorFlags`] которые описывают
        /// один дескриптор сегмента состояния задачи.
        const TSS_DESCRIPTOR_WIDTH: usize =
            mem::size_of::<TssEntry>() / mem::size_of::<DescriptorFlags>();

        let cpu = usize::from(cpu);
        assert!(cpu < TSS_COUNT);

        SegmentSelector::new(
            (BASIC_COUNT + cpu * TSS_DESCRIPTOR_WIDTH).try_into().unwrap(),
            PrivilegeLevel::Ring0,
        )
    }

    /// Возвращает селектор сегмента кода ядра.
    pub(crate) fn kernel_code() -> SegmentSelector {
        Self::basic_selector(DescriptorFlags::KERNEL_CODE64)
    }

    /// Возвращает селектор сегмента данных ядра.
    pub(crate) fn kernel_data() -> SegmentSelector {
        Self::basic_selector(DescriptorFlags::KERNEL_DATA)
    }

    /// Возвращает селектор сегмента кода пользователя.
    pub(crate) fn user_code() -> SegmentSelector {
        Self::basic_selector(DescriptorFlags::USER_CODE64)
    }

    /// Возвращает селектор сегмента данных пользователя.
    pub(crate) fn user_data() -> SegmentSelector {
        Self::basic_selector(DescriptorFlags::USER_DATA)
    }

    /// Загружает GDT в регистр процессора
    /// [`GDTR`](https://wiki.osdev.org/GDT#GDTR).
    /// И инициализирует сегментные регистры `CS`, `DS`, `ES` и `SS`
    /// селекторами сегментов кода и данных ядра из загруженной GDT.
    pub(crate) fn load(&self) {
        let pseudo_descriptor = DescriptorTablePointer {
            limit: (mem::size_of::<Self>() - 1).try_into().expect("too large GDT"),
            base: Virt::from_ref(self).into(),
        };

        unsafe {
            tables::lgdt(&pseudo_descriptor);

            CS::set_reg(Self::kernel_code());

            DS::set_reg(Self::kernel_data());
            ES::set_reg(Self::kernel_data());
            SS::set_reg(Self::kernel_data());
        }
    }

    /// Возвращает один из базовых селекторов для
    /// кода/данных ядра/пользователя, соответствующий заданным `flags`.
    /// То есть, в зависимости от установленных в них битов
    /// [`DescriptorFlags::EXECUTABLE`] и [`DescriptorFlags::DPL_RING_3`].
    fn basic_selector(flags: DescriptorFlags) -> SegmentSelector {
        let is_executable_equal_user = flags.contains(DescriptorFlags::EXECUTABLE) ==
            flags.contains(DescriptorFlags::DPL_RING_3);
        let index: u16 =
            // For the mandatory null descriptor.
            1 +
            if flags.contains(DescriptorFlags::DPL_RING_3) { 2 } else { 0 } +
            // This strange order is for the Star register.
            if is_executable_equal_user { 1 } else { 0 };

        assert!(usize::from(index) < BASIC_COUNT);

        SegmentSelector::new(
            index,
            if flags.contains(DescriptorFlags::DPL_RING_3) {
                PrivilegeLevel::Ring3
            } else {
                PrivilegeLevel::Ring0
            },
        )
    }
}

/// Основная используемая глобальная таблица дескрипторов
/// ([Global Descriptor Table](https://en.wikipedia.org/wiki/Global_Descriptor_Table), GDT).
pub(crate) type Gdt = SmpGdt<MAX_CPUS>;

/// Глобальная таблица дескрипторов
/// ([Global Descriptor Table](https://en.wikipedia.org/wiki/Global_Descriptor_Table), GDT),
/// которая используется только во время старта дополнительных процессоров ---
/// Application Processor --- в [`kernel::smp`].
pub(crate) type SmallGdt = SmpGdt<0>;

/// Количество основных сегментов (нулевой и четыре сегмента для кода/данных ядра/пользователя).
const BASIC_COUNT: usize = 5;

/// Максимальное поддерживаемое количество CPU.
const MAX_CPUS: usize = CpuId::MAX as usize + 1;

/// Запись в таблице дескрипторов для дескриптора сегмента состояния задачи
/// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment), TSS).
/// Занимает два поля типа [`DescriptorFlags`], то есть 128 бит.
/// В отличие от остальных записей таблицы дескрипторов,
/// которые имеют размер 64 бит и описываются одним полем типа [`DescriptorFlags`].
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(C, packed)]
struct TssEntry(DescriptorFlags, DescriptorFlags);

impl TssEntry {
    /// Возвращает дескриптор для заданного `tss`.
    fn new(tss: &TaskStateSegment) -> Self {
        /// Свободный сегмент состояния задачи.
        const AVAILABLE_TSS: u64 = 0b1001 << 40;

        let base = Virt::from_ref(tss).into_u64();
        let limit = size::into_u64(mem::size_of::<TaskStateSegment>() - 1);

        Self(
            DescriptorFlags::from_bits(
                limit |
                    ((base & ((1 << 24) - 1)) << 16) |
                    AVAILABLE_TSS |
                    DescriptorFlags::PRESENT.bits() |
                    ((base >> 24) << 56),
            )
            .unwrap(),
            DescriptorFlags::from_bits(base >> 32).unwrap(),
        )
    }
}

impl Default for TssEntry {
    fn default() -> Self {
        Self(DescriptorFlags::empty(), DescriptorFlags::empty())
    }
}

lazy_static! {
    /// Основная используемая глобальная таблица дескрипторов
    /// ([Global Descriptor Table](https://en.wikipedia.org/wiki/Global_Descriptor_Table), GDT).
    pub(crate) static ref GDT: Spinlock<Gdt> = Spinlock::new({
        let mut gdt = Gdt::new();

        gdt.set_tss(0, &TSS);

        gdt
    });
}
