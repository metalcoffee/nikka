use core::{
    fmt,
    ops::{
        AddAssign,
        Sub,
    },
};

use crate::error::Result;

/// Заглушка на случай выключенной опции `allocator-statistics`.
#[derive(Clone, Copy, Debug, Default, Eq)]
pub struct Info;

impl Info {
    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub(super) const fn new() -> Self {
        Self
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn allocated(&self) -> Counter {
        Counter
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn allocations(&self) -> Counter {
        Counter
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn pages(&self) -> Counter {
        Counter
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn pages_hwm(&self) -> usize {
        0
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn requested(&self) -> Counter {
        Counter
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn fragmentation_loss(&self) -> usize {
        0
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn fragmentation_loss_percentage(&self) -> f64 {
        0.0
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        true
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn allocation(
        &mut self,
        _requested: usize,
        _allocated: usize,
        _allocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn deallocation(
        &mut self,
        _requested: usize,
        _deallocated: usize,
        _deallocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn pages_allocation(
        &mut self,
        _allocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn pages_deallocation(
        &mut self,
        _allocated_pages: usize,
    ) {
    }

    /// Поддерживается ли статистика аллокатора общего назначения.
    /// Равно `true`, если включена опция `allocator-statistics`.
    pub const IS_SUPPORTED: bool = false;
}

impl PartialEq for Info {
    #[inline(always)]
    fn eq(
        &self,
        _other: &Self,
    ) -> bool {
        true
    }
}

impl AddAssign for Info {
    #[inline(always)]
    fn add_assign(
        &mut self,
        _other: Info,
    ) {
    }
}

impl Sub<Info> for Info {
    type Output = Result<Self>;

    #[inline(always)]
    fn sub(
        self,
        _rhs: Info,
    ) -> Self::Output {
        Ok(Self)
    }
}

/// Заглушка на случай выключенной опции `allocator-statistics`.
#[derive(Debug, Default)]
pub struct AtomicInfo;

impl AtomicInfo {
    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn new() -> Self {
        Self
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn allocation(
        &self,
        _requested: usize,
        _allocated: usize,
        _allocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn deallocation(
        &self,
        _requested: usize,
        _deallocated: usize,
        _deallocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn pages_allocation(
        &self,
        _allocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn pages_deallocation(
        &self,
        _allocated_pages: usize,
    ) {
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn load(&self) -> Info {
        Info
    }
}

impl fmt::Display for Info {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "{{ \"allocator-statistics\" feature is disabled }}",
        )
    }
}

/// Заглушка на случай выключенной опции `allocator-statistics`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counter;

impl Counter {
    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn negative(&self) -> usize {
        0
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn positive(&self) -> usize {
        0
    }

    /// Заглушка на случай выключенной опции `allocator-statistics`.
    #[inline(always)]
    pub const fn balance(&self) -> usize {
        0
    }
}
