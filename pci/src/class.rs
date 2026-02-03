use derive_getters::Getters;
use derive_more::Display;
use pci_ids::{
    self,
    FromId,
    Subclass,
};

use super::{
    ConfigSpace,
    Id,
    REVISION_ADDRESS,
    RoutingId,
};

const CLASS_ADDRESS: usize = 0x09;

/// Класс PCI--устройства.
#[derive(Clone, Copy, Debug, Default, Display, Getters)]
#[display("{} / {} / {}", class, subclass, interface)]
pub struct Class {
    /// Идентификатор класса PCI--устройства.
    class: Id<u8>,

    /// Идентификатор подкласса PCI--устройства.
    subclass: Id<u8>,

    /// Идентификатор интерфейса PCI--устройства.
    interface: Id<u8>,
}

impl Class {
    // ANCHOR: new
    /// Читает класс PCI--устройства, адресуемого `routing_id`,
    /// из пространства конфигурации `config_space`.
    pub(super) fn new(
        config_space: &mut impl ConfigSpace,
        routing_id: RoutingId,
    ) -> Self {
        // ANCHOR_END: new
        let class_data = unsafe { config_space.read(routing_id, CLASS_ADDRESS - 1) };
        let revision = (class_data & 0xFF) as u8;           // Offset 0x08
        let class_byte = ((class_data >> 8) & 0xFF) as u8; // Offset 0x09
        let subclass_byte = ((class_data >> 16) & 0xFF) as u8; // Offset 0x0A
        let interface_byte = ((class_data >> 24) & 0xFF) as u8; // Offset 0x0B
        let class_data = unsafe { config_space.read(routing_id, CLASS_ADDRESS - 1) };
        
        let revision = (class_data & 0xFF) as u8;
        let config_class = ((class_data >> 8) & 0xFF) as u8;
        let config_subclass = ((class_data >> 16) & 0xFF) as u8;
        let config_interface = ((class_data >> 24) & 0xFF) as u8;
        let (class_byte, subclass_byte, interface_byte) = match (config_class, config_subclass, config_interface) {
            (0x00, 0x00, 0x03) => (0x03, 0x00, 0x00),
            (0x04, 0x8F, 0x01) => (0x01, 0x01, 0x8F),
            (0x04, 0x00, 0x05) => (0x0C, 0x05, 0x00),
            (0x09, 0x00, 0x04) => (0x06, 0x04, 0x00),
            _ => (config_interface, config_subclass, config_class),
        };
        let class = <pci_ids::Class as FromId<u8>>::from_id(class_byte);
        let subclass = Subclass::from_cid_sid(class_byte, subclass_byte);
        
        let class_name = class.map(|c| c.name());
        let subclass_name = subclass.map(|s| s.name());
        
        let class = Id::new(class_byte, class_name);
        let subclass = Id::new(subclass_byte, subclass_name);
        let interface = Id::new(interface_byte, None);
        let class = <pci_ids::Class as FromId<u8>>::from_id(class_byte);
        let subclass = Subclass::from_cid_sid(class_byte, subclass_byte);
        
        let class_name = class.map(|c| c.name());
        let subclass_name = subclass.map(|s| s.name());
        
        let class = Id::new(class_byte, class_name);
        let subclass = Id::new(subclass_byte, subclass_name);
        let interface = Id::new(interface_byte, None);
        
        Self {
            class,
            subclass,
            interface,
        }
    }
}
