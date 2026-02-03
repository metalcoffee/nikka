#![deny(warnings)]

use tracing_core::LevelFilter;
use tracing_subscriber::{
    self,
    EnvFilter,
    fmt,
};

pub fn init() {
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
