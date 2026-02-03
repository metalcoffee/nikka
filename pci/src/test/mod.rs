use tracing_core::LevelFilter;
use tracing_subscriber::{
    self,
    EnvFilter,
    fmt,
};

use ku::log::debug;

use mock_device::MockDevice;

mod devices;
mod mock_device;

#[test]
fn device_id() {
    for mut device in devices::all() {
        debug!(device = device.name());
        device.validate_device();
    }
}

#[test]
fn class() {
    for mut device in devices::all() {
        debug!(device = device.name());
        device.validate_class();
    }
}

#[test]
fn bar() {
    for mut device in devices::normal() {
        debug!(device = device.name());
        device.validate_bars();
    }
}

#[test]
fn normal() {
    for mut device in devices::normal() {
        debug!(device = device.name());
        device.validate();
    }
}

#[test]
fn bridge() {
    for mut device in devices::bridge() {
        debug!(device = device.name());
        device.validate();
    }
}

#[ctor::ctor]
fn init() {
    let filter = EnvFilter::from_default_env().add_directive(LevelFilter::DEBUG.into());

    let format = fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(true)
        .compact();

    tracing_subscriber::fmt()
        .with_ansi(false)
        .event_format(format)
        .with_env_filter(filter)
        .init();
}
