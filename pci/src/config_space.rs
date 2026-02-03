use ku::memory::{
    IndexDataPair,
    IndexDataPortPair,
};

use super::RoutingId;

/// Типаж для работы с
/// [пространством конфигурации PCI](https://en.wikipedia.org/wiki/PCI_configuration_space).
pub trait ConfigSpace {
    /// Читает 32-битную величину по смещению `offset`
    /// в пространстве конфигурации устройства, адресуемого `routing_id`.
    ///
    /// # Safety
    ///
    /// Определяется спецификацией шины и устройств PCI.
    unsafe fn read(
        &mut self,
        routing_id: RoutingId,
        offset: usize,
    ) -> u32;

    /// Записывает 32-битную величину по смещению `offset`
    /// в пространстве конфигурации устройства, адресуемого `routing_id`.
    ///
    /// # Safety
    ///
    /// Определяется спецификацией шины и устройств PCI.
    unsafe fn write(
        &mut self,
        routing_id: RoutingId,
        offset: usize,
        data: u32,
    );
}

/// Структура для работы с
/// [пространством конфигурации PCI](https://en.wikipedia.org/wiki/PCI_configuration_space)
/// через
/// [порты ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
pub struct PortConfigSpace(IndexDataPortPair<u32>);

impl PortConfigSpace {
    /// Создаёт структуру для чтения пространства конфигурации PCI через
    /// [порты ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    pub fn new() -> Self {
        let ports = IndexDataPortPair::from_index_port(Self::INDEX_PORT)
            .expect("invalid PCI configuration space index port");

        Self(ports)
    }

    /// Возвращает индекс, который нужно записать в индексный порт
    /// для чтения или записи 32-битной величины по смещению `offset`
    /// в пространстве конфигурации устройства, адресуемого `routing_id`.
    fn index(
        routing_id: RoutingId,
        offset: usize,
    ) -> u32 {
        assert_eq!(offset % 4, 0);
        let offset = u8::try_from(offset)
            .expect("out-of-bounds access to port-based PCI configuration space");

        0x8000_0000 |
            u32::from(routing_id.bus()) << 16 |
            u32::from(routing_id.device()) << 11 |
            u32::from(routing_id.function()) << 8 |
            u32::from(offset)
    }

    /// Номер индексного порта при обращении к конфигурации PCI.
    const INDEX_PORT: u16 = 0x0CF8;
}

impl ConfigSpace for PortConfigSpace {
    unsafe fn read(
        &mut self,
        routing_id: RoutingId,
        offset: usize,
    ) -> u32 {
        u32::from_le(unsafe { self.0.read(Self::index(routing_id, offset)) })
    }

    unsafe fn write(
        &mut self,
        routing_id: RoutingId,
        offset: usize,
        data: u32,
    ) {
        unsafe { self.0.write(Self::index(routing_id, offset), data.to_le()) };
    }
}

impl Default for PortConfigSpace {
    fn default() -> Self {
        Self::new()
    }
}
