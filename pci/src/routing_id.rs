use derive_more::Display;

/// Географические координаты PCI--устройства.
#[derive(Clone, Copy, Debug, Display)]
#[display("{:02x}:{:02x}.{:01x}", bus, device, function)]
pub struct RoutingId {
    /// Номер шины.
    bus: u8,

    /// Номер устройства на шине,
    /// должен быть от `0` до [`RoutingId::MAX_DEVICE_COUNT`] не включительно.
    device: u8,

    /// Номер функции в устройстве,
    /// должен быть от `0` до [`RoutingId::MAX_FUNCTION_COUNT`] не включительно.
    function: u8,
}

impl RoutingId {
    /// Создаёт географические координаты PCI--устройства.
    ///
    /// Аргументы:
    /// - `bus` --- номер шины.
    /// - `device` --- номер устройства на шине,
    ///   должен быть от `0` до [`RoutingId::MAX_DEVICE_COUNT`] не включительно.
    /// - `function` --- номер функции в устройстве,
    ///   должен быть от `0` до [`RoutingId::MAX_FUNCTION_COUNT`] не включительно.
    ///
    /// # Panics
    ///
    /// Паникует, если `device` или `function` выходят за допустимые пределы.
    pub fn new(
        bus: u8,
        device: u8,
        function: u8,
    ) -> Self {
        assert!(device < Self::MAX_DEVICE_COUNT);
        assert!(function < Self::MAX_FUNCTION_COUNT);

        Self {
            bus,
            device,
            function,
        }
    }

    /// Возвращает номер шины.
    pub fn bus(&self) -> u8 {
        self.bus
    }

    /// Возвращает номер устройства на шине.
    pub fn device(&self) -> u8 {
        self.device
    }

    /// Возвращает номер функции в устройстве,
    pub fn function(&self) -> u8 {
        self.function
    }

    /// Максимальное количество устройств на одной шине.
    pub const MAX_DEVICE_COUNT: u8 = 1 << 5;

    /// Максимальное количество функции в одном устройстве.
    pub const MAX_FUNCTION_COUNT: u8 = 1 << 3;
}
