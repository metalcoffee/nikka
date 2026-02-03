use core::mem;

use derive_getters::Getters;
use derive_more::Display;

use ku::memory::{
    Block,
    KiB,
    MiB,
    Phys,
    Port,
    size,
};

use super::{
    BARS_END_ADDRESS,
    BARS_START_ADDRESS,
    Bar,
    Class,
    ConfigSpace,
    DeviceId,
    Id,
    RoutingId,
    bar::Width,
    vendor_device,
};

/// Тип PCI--устройства.
#[derive(Clone, Copy, Debug, Display)]
pub enum Kind {
    /// Обычное (конечное) PCI--устройство.
    #[display("Normal device")]
    Normal {
        /// Регистры адресов памяти и
        /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
        /// PCI--устройства --- Base Address Registers (BARs).
        bars: [Option<Bar>; 6],
    },

    /// PCI--PCI мост.
    #[display(
        "PCI-to-PCI Bridge {{ primary: {:02X}, secondary: {:02X}, subordinate: {:02X} }}",
        primary,
        secondary,
        subordinate
    )]
    PciPciBridge {
        /// Регистры адресов памяти и
        /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
        /// PCI--PCI моста --- Base Address Registers (BARs).
        bars: [Option<Bar>; 2],

        /// Номер шины, расположенной непосредственно перед PCI--PCI мостом.
        primary: u8,

        /// Номер шины, расположенной непосредственно после PCI--PCI моста.
        secondary: u8,

        /// Наибольший номер шины из всех шин,
        /// к которым возможен доступ через PCI--PCI мост.
        subordinate: u8,

        /// Диапазон
        /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
        /// для всех PCI--устройств за PCI--PCI мостом.
        io_behind_bridge: Block<Port<u8>>,

        /// Диапазон физической памяти для
        /// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
        /// всех PCI--устройств за PCI--PCI мостом.
        memory_behind_bridge: Block<Phys>,

        /// Диапазон физической памяти для
        /// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
        /// всех PCI--устройств за PCI--PCI мостом,
        /// для которой разрешено упреждающее чтение.
        prefetchable_memory_behind_bridge: Block<Phys>,
    },

    /// PCI--[CardBus](https://en.wikipedia.org/wiki/PC_Card#CardBus) мост.
    #[display("PCI-to-CardBus Bridge")]
    CardBusBridge,

    /// Неизвестное устройство.
    Unknown,
}

/// Структура, описывающая PCI--устройство.
#[derive(Clone, Copy, Debug, Display, Getters)]
#[display(
    "{{ vendor: {}, device: {}, revision: {}, class: {}, subclass: {}, interface: {} }}",
    id.vendor(),
    id.device(),
    id.revision(),
    class.class(),
    class.subclass(),
    class.interface()
)]
pub struct Device {
    /// Класс PCI--устройства.
    class: Class,

    /// Идентификатор PCI--устройства.
    id: DeviceId,

    /// Поддерживает ли устройство несколько функций.
    is_multi_function: bool,

    /// Тип PCI--устройства.
    kind: Kind,

    /// Идентификатор подустройства.
    /// Например, конкретной платы, основанной на микросхеме,
    /// задаваемой основным идентификатором устройства.
    subvendor: Option<Id<u16>>,

    /// Идентификатор производителя подустройства.
    /// Например, производителя конкретной платы, основанной на микросхеме,
    /// задаваемой основным идентификатором устройства.
    subdevice: Option<Id<u16>>,
}

impl Device {
    // ANCHOR: new
    /// Читает описание PCI--устройства, адресуемого `routing_id`,
    /// из пространства конфигурации `config_space`.
    /// Если по адресу `routing_id` нет устройства, возвращает [`None`].
    pub fn new(
        config_space: &mut impl ConfigSpace,
        routing_id: RoutingId,
    ) -> Option<Self> {
        // ANCHOR_END: new
        let id = DeviceId::new(config_space, routing_id)?;
        let class = Class::new(config_space, routing_id);
        let header_type_data = unsafe { config_space.read(routing_id, HEADER_TYPE_ADDRESS & !0x3) };
        let header_type = (header_type_data >> ((HEADER_TYPE_ADDRESS & 0x3) * 8)) as u8;
        let is_multi_function = (header_type & 0x80) != 0;
        let header_type = header_type & 0x7F;
        let subvendor_subdevice_data = unsafe { config_space.read(routing_id, SUBSYSTEM_VENDOR_ID_ADDRESS) };
        let (subdevice, subvendor) = vendor_device(subvendor_subdevice_data).unwrap_or((Id::new(0, None), Id::new(0, None)));

        let kind = match header_type {
            0x00 => Self::read_normal(config_space, routing_id),
            0x01 => Self::read_bridge(config_space, routing_id),
            0x02 => Kind::CardBusBridge,
            _ => Kind::Unknown,
        };

        Some(Self {
            class,
            id,
            is_multi_function,
            kind,
            subvendor: if subvendor.id() == 0 { None } else { Some(subvendor) },
            subdevice: if subdevice.id() == 0 { None } else { Some(subdevice) },
        })
    }

    /// Возвращает тип [`Kind`] обычного PCI--устройства, адресуемого `routing_id`,
    /// из пространства конфигурации `config_space`.
    fn read_normal(
        config_space: &mut impl ConfigSpace,
        routing_id: RoutingId,
    ) -> Kind {
        let bars = Self::read_bars(config_space, routing_id);

        assert_eq!(
            bars.len(),
            (BARS_END_ADDRESS - BARS_START_ADDRESS) / mem::size_of::<u32>(),
        );

        Kind::Normal { bars }
    }

    // ANCHOR: read_bridge
    /// Возвращает тип [`Kind`] PCI--PCI моста, адресуемого `routing_id`,
    /// из пространства конфигурации `config_space`.
    fn read_bridge(
        config_space: &mut impl ConfigSpace,
        routing_id: RoutingId,
    ) -> Kind {
        // ANCHOR_END: read_bridge
        let bus_numbers_data = unsafe { config_space.read(routing_id, PRIMARY_BUS_NUMBER_ADDRESS & !0x3) };
        let primary = (bus_numbers_data & 0xFF) as u8;
        let secondary = ((bus_numbers_data >> 8) & 0xFF) as u8;
        let subordinate = ((bus_numbers_data >> 16) & 0xFF) as u8;
        let io_base_limit_data = unsafe { config_space.read(routing_id, IO_BASE_ADDRESS & !0x3) };
        let io_base = (io_base_limit_data & 0xFF) as u8;
        let io_limit = ((io_base_limit_data >> 8) & 0xFF) as u8;

        let io_behind_bridge = if io_base <= io_limit {
            let start = (io_base as usize) << 8;
            let end = ((io_limit as usize) << 8) | 0xFFF;
            Block::from_index(start, end + 1).unwrap_or_default()
        } else {
            Block::default()
        };
        let memory_base_limit_data = unsafe { config_space.read(routing_id, MEMORY_BASE_ADDRESS & !0x3) };
        let memory_base = (memory_base_limit_data & 0xFFFF) as u16;
        let memory_limit = ((memory_base_limit_data >> 16) & 0xFFFF) as u16;
        let memory_behind_bridge = if memory_base <= memory_limit {
            let start = (memory_base as usize) << 16;
            let end = ((memory_limit as usize) << 16) | 0xFFFFF;
            Block::from_index(start, end + 1).unwrap_or_default()
        } else {
            Block::default()
        };
        let prefetchable_memory_base_limit_data = unsafe { config_space.read(routing_id, PREFETCHABLE_MEMORY_BASE_ADDRESS & !0x3) };
        let prefetchable_memory_base = (prefetchable_memory_base_limit_data & 0xFFFF) as u16;
        let prefetchable_memory_limit = ((prefetchable_memory_base_limit_data >> 16) & 0xFFFF) as u16;
        let prefetchable_memory_type = unsafe { config_space.read(routing_id, PREFETCHABLE_MEMORY_BASE_ADDRESS + 4) };
        let is_64bit_prefetchable = (prefetchable_memory_type & PREFETCHABLE_MEMORY_TYPE_MASK) == PREFETCHABLE_MEMORY_TYPE_64;
        let prefetchable_memory_behind_bridge = if prefetchable_memory_base <= prefetchable_memory_limit {
            if is_64bit_prefetchable {
                // Read upper 32 bits
                let prefetchable_memory_base_upper = unsafe { config_space.read(routing_id, PREFETCHABLE_MEMORY_BASE_UPPER_ADDRESS) };
                let prefetchable_memory_limit_upper = unsafe { config_space.read(routing_id, PREFETCHABLE_MEMORY_LIMIT_UPPER_ADDRESS) };
                
                let start = ((prefetchable_memory_base_upper as u64) << 32) | (((prefetchable_memory_base & 0xFFF0) as u64) << 16);
                let end = ((prefetchable_memory_limit_upper as u64) << 32) | (((prefetchable_memory_limit & 0xFFF0) as u64) << 16) | 0xFFFFF;
                Block::from_index(start as usize, (end + 1) as usize).unwrap_or_default()
            } else {
                let start = ((prefetchable_memory_base & 0xFFF0) as usize) << 16;
                let end = ((prefetchable_memory_limit & 0xFFF0) as usize) << 16 | 0xFFFFF;
                Block::from_index(start, end + 1).unwrap_or_default()
            }
        } else {
            Block::default()
        };
        let bars = Self::read_bars::<2>(config_space, routing_id);
        
        Kind::PciPciBridge {
            bars,
            primary,
            secondary,
            subordinate,
            io_behind_bridge,
            memory_behind_bridge,
            prefetchable_memory_behind_bridge,
        }
    }

    // ANCHOR: read_bars
    /// Читает `N` регистров адресов памяти и
    /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
    /// --- Base Address Registers (BARs) ---
    /// PCI--устройства, адресуемого `routing_id`,
    /// из пространства конфигурации `config_space`.
    fn read_bars<const N: usize>(
        config_space: &mut impl ConfigSpace,
        routing_id: RoutingId,
    ) -> [Option<Bar>; N] {
        // ANCHOR_END: read_bars
        let mut bars = [None; N];
        let mut i = 0;
        let mut offset = BARS_START_ADDRESS;
        
        while i < N && offset < BARS_END_ADDRESS {
            if let Some(bar) = Bar::new(config_space, routing_id, offset) {
                bars[i] = Some(bar);
                
                // Check if this is a 64-bit memory BAR, which takes two slots
                if let Some(Bar::Memory { width: Width::Memory64, .. }) = &bars[i] {
                    offset += 8; // Skip next 4 bytes as they're part of this 64-bit BAR
                } else {
                    offset += 4;
                }
                i += 1;
            } else {
                bars[i] = None;
                offset += 4;
                i += 1;
            }
        }
        
        bars
    }
}

/// Смещение регистра типа заголовка в пространстве конфигурации PCI--устройства.
const HEADER_TYPE_ADDRESS: usize = 0x0E;

/// Смещение регистра размера линии кэша в пространстве конфигурации PCI--устройства.
const CACHE_LINE_SIZE_ADDRESS: usize = 0x0C;

/// Смещение идентификатора производителя подустройства
/// в пространстве конфигурации PCI--устройства.
const SUBSYSTEM_VENDOR_ID_ADDRESS: usize = 0x2C;

/// Смещение регистра номера шины в пространстве конфигурации моста PCI--PCI.
const PRIMARY_BUS_NUMBER_ADDRESS: usize = 0x18;

/// Смещение регистра младших разрядов диапазона
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
/// в пространстве конфигурации моста PCI--PCI.
const IO_BASE_ADDRESS: usize = 0x1C;

/// Смещение регистра старших разрядов диапазона
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
/// в пространстве конфигурации моста PCI--PCI.
const IO_BASE_UPPER_ADDRESS: usize = 0x30;

/// Гранулярность регистров диапазона
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
const BRIDGE_IO_GRANULARITY: usize = 4 * KiB;

/// Смещение регистра диапазона физической памяти для
/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O)
/// в пространстве конфигурации моста PCI--PCI.
const MEMORY_BASE_ADDRESS: usize = 0x20;

/// Гранулярность регистров диапазона физической памяти для
/// в пространстве конфигурации моста PCI--PCI.
const BRIDGE_MEMORY_GRANULARITY: usize = MiB;

/// Смещение регистра младших битов диапазона физической памяти для
/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O),
/// для которой разрешено упреждающее чтение,
/// в пространстве конфигурации моста PCI--PCI.
const PREFETCHABLE_MEMORY_BASE_ADDRESS: usize = 0x24;

/// Смещение регистра старших битов адреса первого байта физической памяти для
/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O),
/// для которой разрешено упреждающее чтение,
/// в пространстве конфигурации моста PCI--PCI.
const PREFETCHABLE_MEMORY_BASE_UPPER_ADDRESS: usize = 0x28;

/// Смещение регистра старших битов адреса последнего байта физической памяти для
/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O),
/// для которой разрешено упреждающее чтение,
/// в пространстве конфигурации моста PCI--PCI.
const PREFETCHABLE_MEMORY_LIMIT_UPPER_ADDRESS: usize = 0x2C;

/// Маска ширины адресов физической памяти для
/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O),
/// для которой разрешено упреждающее чтение.
const PREFETCHABLE_MEMORY_TYPE_MASK: u32 = 0xF_0000;

/// Признак 64-разрядности адресов физической памяти для
/// [Memory--mapped I/O (MMIO)](https://en.wikipedia.org/wiki/Memory-mapped_I/O),
/// для которой разрешено упреждающее чтение.
const PREFETCHABLE_MEMORY_TYPE_64: u32 = 0x1_0000;

/// Смещение следующего за последним BAR--регистром в пространстве конфигурации моста PCI--PCI.
const BARS_END_ADDRESS_BRIDGE: usize = 0x18;
