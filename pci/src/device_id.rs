use derive_getters::Getters;
use derive_more::Display;
use pci_ids::{
    Device,
    FromId,
    Vendor,
};

use super::{
    ConfigSpace,
    Id,
    RoutingId,
};

/// Идентификатор PCI--устройства.
#[derive(Clone, Copy, Debug, Default, Display, Getters)]
#[display("{}, rev. {} ({})", device, revision, vendor)]
pub struct DeviceId {
    /// Идентификатор устройства.
    device: Id<u16>,

    /// Идентификатор производителя устройства.
    vendor: Id<u16>,

    /// Номер ревизии устройства.
    revision: u8,
}

impl DeviceId {
    // ANCHOR: new
    /// Читает идентификатор PCI--устройства, адресуемого `routing_id`,
    /// из пространства конфигурации `config_space`.
    /// Если по адресу `routing_id` нет устройства, возвращает [`None`].
    pub(super) fn new(
        config_space: &mut impl ConfigSpace,
        routing_id: RoutingId,
    ) -> Option<Self> {
        // ANCHOR_END: new
        let vendor_device_data = unsafe { config_space.read(routing_id, VENDOR_ID_ADDRESS) };
        let (device, vendor) = vendor_device(vendor_device_data)?;
        let revision_data = unsafe { config_space.read(routing_id, REVISION_ADDRESS) };
        let revision = (revision_data & 0xFF) as u8;
        Some(Self {
            device,
            vendor,
            revision,
        })
    }
}

// ANCHOR: vendor_device
/// Возвращает пару из идентификатора устройства и идентификатора производителя устройства,
/// закодированные в значении `data`.
/// Также подходит для декодирования пары из
/// идентификатора подустройства и идентификатора производителя подустройства.
pub(super) fn vendor_device(data: u32) -> Option<(Id<u16>, Id<u16>)> {
    // ANCHOR_END: vendor_device
    let vendor_id = (data & 0xFFFF) as u16;
    let device_id = ((data >> 16) & 0xFFFF) as u16;
    
    if vendor_id == NO_DEVICE {
        return None;
    }
    let vendor = Vendor::from_id(vendor_id);
    let device = Device::from_vid_pid(vendor_id, device_id);
    
    let vendor_name = vendor.map(|v| v.name());
    let device_name = device.map(|d| d.name());
    
    let vendor_id = Id::new(vendor_id, vendor_name);
    let device_id = Id::new(device_id, device_name);
    
    Some((device_id, vendor_id))
}

/// Смещение номера ревизии устройства в пространстве конфигурации PCI--устройства.
pub(super) const REVISION_ADDRESS: usize = 0x08;

/// Смещение идентификатора производителя устройства в пространстве конфигурации PCI--устройства.
const VENDOR_ID_ADDRESS: usize = 0x00;

/// Признак отсутствия устройства.
const NO_DEVICE: u16 = u16::MAX;
