#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

//! Библиотека для работы с конфигурацией шин
//! [PCI (Peripheral Component Interconnect)](https://en.wikipedia.org/wiki/Peripheral_Component_Interconnect)
//! и
//! [PCI Express (Peripheral Component Interconnect Express)](https://en.wikipedia.org/wiki/PCI_Express).

#![deny(warnings)]
#![feature(trait_alias)]
#![no_std]
#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]

pub use bar::{
    Bar,
    Width,
};
pub use class::Class;
pub use config_space::{
    ConfigSpace,
    PortConfigSpace,
};
pub use device::{
    Device,
    Kind,
};
pub use device_id::DeviceId;
pub use id::Id;
pub use routing_id::RoutingId;

use bar::{
    BARS_END_ADDRESS,
    BARS_START_ADDRESS,
};
use device_id::{
    REVISION_ADDRESS,
    vendor_device,
};

/// Модуль для работы с регистрами адресов памяти и
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
/// PCI--устройств --- Base Address Register (BAR).
mod bar;

/// Класс PCI--устройства.
mod class;

/// Модуль для работы с
/// [пространством конфигурации PCI](https://en.wikipedia.org/wiki/PCI_configuration_space).
mod config_space;

/// Модуль для работы с описанием PCI--устройство.
mod device;

/// Идентификатор PCI--устройства.
mod device_id;

/// Единый тип для идентификаторов PCI устройств, производителей, классов и т.д.
mod id;

/// Географические координаты PCI--устройства.
mod routing_id;

#[cfg(test)]
mod test;
