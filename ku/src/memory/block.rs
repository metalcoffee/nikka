use core::{
    alloc::Layout,
    cmp::{
        self,
        Ordering,
        PartialOrd,
    },
    fmt,
    iter::IntoIterator,
    marker::PhantomData,
    mem,
    ops::Range,
    option::Option,
    ptr::NonNull,
};

use serde::{
    Deserialize,
    Serialize,
};

use crate::error::{
    Error::{
        InvalidArgument,
        Overflow,
    },
    Result,
};

use super::{
    addr::{
        Addr,
        GroupedHex,
        IsVirt,
        Tag,
        Virt,
    },
    frage::{
        Frage,
        L0_SIZE,
    },
    size,
    size::{
        Size,
        SizeOf,
    },
};

// Used in docs.
#[allow(unused)]
use {
    super::{
        Frame,
        Page,
        Phys,
    },
    crate::error::Error,
};

/// Абстракция куска физической или виртуальной памяти, постраничного или произвольного.
///
/// - [`Block<Phys>`] --- произвольный кусок физической памяти.
/// - [`Block<Virt>`] --- произвольный кусок виртуальной памяти.
/// - [`Block<Frame>`] --- набор последовательных физических фреймов.
/// - [`Block<Page>`] --- набор последовательных виртуальных страниц.
///
/// [`Block`] не владеет описываемой им памятью.
#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Block<T: Memory> {
    /// Номер первого элемента в блоке.
    start: usize,

    /// Номер следующего за последним элементом блока.
    end: usize,

    /// Фантомное, не занимающее памяти, поле.
    /// Служит для того чтобы сделать блоки с разными параметрами `T` несовместимыми.
    tag: PhantomData<T>,
}

impl<T: Memory> Block<T> {
    /// Создаёт блок для полуоткрытого интервала `[start, end)` базового типа `T`,
    /// который может быть [`Phys`], [`Virt`], [`Frame`] или [`Page`].
    pub fn new(
        start: T,
        end: T,
    ) -> Result<Self> {
        Self::from_index(T::index(start), T::index(end))
    }

    /// Создаёт блок для полуоткрытого интервала, начинающегося
    /// с элемента `start` включительно и содержащий `count` элементов типа `T`.
    /// Базовый тип `T` может быть [`Phys`], [`Virt`], [`Frame`] или [`Page`].
    ///
    /// # Safety
    ///
    /// - `start` и `count` не должны приводить к переполнениям.
    /// - Для [`Virt`] и [`Page`] получающийся блок должен целиком лежать в одной
    ///   [половине виртуального адресного пространства](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details).
    pub unsafe fn from_count_unchecked(
        start: T,
        count: usize,
    ) -> Self {
        unsafe { Self::from_index_count_unchecked(T::index(start), count) }
    }

    /// Создаёт блок для полуоткрытого интервала, начинающегося
    /// с элемента с номером `start` включительно и содержащий `count` элементов типа `T`.
    /// Базовый тип `T` может быть [`Phys`], [`Virt`], [`Frame`] или [`Page`].
    ///
    /// # Safety
    ///
    /// - `start` и `count` не должны приводить к переполнениям.
    /// - Для [`Virt`] и [`Page`] получающийся блок должен целиком лежать в одной
    ///   [половине виртуального адресного пространства](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details).
    pub unsafe fn from_index_count_unchecked(
        start: usize,
        count: usize,
    ) -> Self {
        Self {
            start,
            end: start + count,
            tag: PhantomData,
        }
    }

    /// Возвращает пустой [`Block`].
    /// В отличие от [`Block::default()`] доступна в константном контексте.
    pub const fn zero() -> Self {
        Self {
            start: 0,
            end: 0,
            tag: PhantomData,
        }
    }

    /// Создаёт блок для полуоткрытого интервала `[start, end)` базового типа `T`,
    /// который задаётся своими индексами --- номерами байт для [`Phys`] и [`Virt`],
    /// номерами физических фреймов для [`Frame`] и номерами виртуальных страниц для [`Page`].
    pub fn from_index(
        start: usize,
        end: usize,
    ) -> Result<Self> {
        let start_address = T::address_by_index(start)?;
        let last_address = T::address_by_index(
            if start < end {
                end - 1
            } else {
                end
            },
        )?;

        if start <= end && T::is_same_half(start_address, last_address) {
            Ok(Self {
                start,
                end,
                tag: PhantomData,
            })
        } else {
            Err(InvalidArgument)
        }
    }

    /// Создаёт блок для полуоткрытого интервала `[start, end)` базового типа `T`,
    /// который задаётся своими индексами --- номерами байт для [`Phys`] и [`Virt`],
    /// номерами физических фреймов для [`Frame`] и номерами виртуальных страниц для [`Page`].
    ///
    /// Аналогичен [`Block::from_index()`], но индексы имеют тип [`u64`].
    pub fn from_index_u64(
        start: u64,
        end: u64,
    ) -> Result<Self> {
        Self::from_index(size::from(start), size::from(end))
    }

    /// Создаёт блок из одного элемента `element`.
    pub fn from_element(element: T) -> Result<Self> {
        Self::from_index(T::index(element), T::index(element) + 1)
    }

    /// Возвращает количество элементов в блоке, оно равно `Block::end - Block::start`.
    pub const fn count(&self) -> usize {
        self.end - self.start
    }

    /// Возвращает требования к размещению в памяти текущего блока ---
    /// его размер и выравнивание.
    pub const fn layout(&self) -> Layout {
        unsafe {
            assert!(T::SIZE_OF > 0);
            assert!(T::SIZE_OF.is_power_of_two());
            assert!(T::SIZE_OF <= isize::MAX as usize);
            assert!(T::ADDRESS_BITS < isize::BITS - 1);
            assert!(
                (1_usize << T::ADDRESS_BITS).next_multiple_of(T::SIZE_OF) <= isize::MAX as usize,
            );

            Layout::from_size_align_unchecked(self.size(), T::SIZE_OF)
        }
    }

    /// Размер блока в байтах. Равно количеству элементов в блоке, умноженному на размер элемента.
    pub const fn size(&self) -> usize {
        self.count() * T::SIZE_OF
    }

    /// Индекс первого элемента в блоке.
    pub fn start(&self) -> usize {
        self.start
    }

    /// Индекс следующего за последним элементом в блоке.
    pub fn end(&self) -> usize {
        self.end
    }

    /// Возвращает адрес первого элемента в блоке.
    pub fn start_address(&self) -> T::Address {
        unsafe { T::address_by_index_unchecked(self.start) }
    }

    /// Возвращает адрес следующего за последним элементом блока.
    pub fn end_address(&self) -> Result<T::Address> {
        T::address_by_index(self.end)
    }

    /// Возвращает адрес следующего за последним элементом блока.
    pub fn end_address_u128(&self) -> u128 {
        u128::try_from(self.end()).unwrap() * u128::try_from(T::SIZE_OF).unwrap()
    }

    /// Возвращает адрес по смещению `offset` байт внутри блока.
    ///
    /// Возвращает ошибку
    ///   - [`Error::Overflow`] если `offset` выходит за границы блока.
    pub fn address(
        &self,
        offset: usize,
    ) -> Result<T::Address> {
        if offset < self.size() {
            let address = T::address_into_usize(self.start_address()) + offset;

            Ok(T::usize_into_address(address).expect("invalid block"))
        } else {
            Err(Overflow)
        }
    }

    /// Возвращает адрес по смещению `offset` байт внутри блока.
    ///
    /// # Safety
    ///
    /// Параметр `offset` не должен выходить за границы блока.
    pub unsafe fn address_unchecked(
        &self,
        offset: usize,
    ) -> T::Address {
        let address = T::address_into_usize(self.start_address()).wrapping_add(offset);

        unsafe { T::usize_into_address_unchecked(address) }
    }

    /// Возвращает смещение в байтах адреса `address` внутри блока.
    ///
    /// Возвращает ошибку
    ///   - [`Error::Overflow`] если `address` выходит за границы блока.
    pub fn offset(
        &self,
        address: T::Address,
    ) -> Result<usize> {
        if self.contains_address(address) {
            Ok(T::address_into_usize(address) - T::address_into_usize(self.start_address()))
        } else {
            Err(Overflow)
        }
    }

    /// Возвращает смещение в байтах адреса `address` внутри блока.
    ///
    /// # Safety
    ///
    /// Параметр `address` не должен выходить за границы блока.
    pub unsafe fn offset_unchecked(
        &self,
        address: T::Address,
    ) -> usize {
        T::address_into_usize(address) - T::address_into_usize(self.start_address())
    }

    /// Возвращает первый элемент в блоке.
    pub fn start_element(&self) -> T {
        T::from_index(self.start).unwrap()
    }

    /// Возвращает следующий за последним элементом блока.
    pub fn end_element(&self) -> Result<T> {
        T::from_index(self.end)
    }

    /// Проверяет, что заданный элемент относится к блоку.
    pub fn contains(
        &self,
        element: T,
    ) -> bool {
        self.contains_address(T::address(element))
    }

    /// Проверяет, что заданный адрес относится к блоку.
    pub fn contains_address(
        &self,
        addr: T::Address,
    ) -> bool {
        if addr < self.start_address() {
            false
        } else if let Ok(end_address) = self.end_address() {
            addr < end_address
        } else {
            true
        }
    }

    /// Проверяет, что заданный `block` целиком содержится внутри текущего или совпадает с ним.
    pub fn contains_block(
        &self,
        block: Self,
    ) -> bool {
        self.start <= block.start && block.end <= self.end
    }

    /// Проверяет, что элемент с заданным индексом `index` относится к блоку.
    pub fn contains_index(
        &self,
        index: usize,
    ) -> bool {
        self.start <= index && index < self.end
    }

    /// Проверяет, что заданный `block` лежит правее и является смежным с текущим.
    ///
    /// См. также [`Block::coalesce()`].
    pub fn is_adjacent(
        &self,
        block: Self,
    ) -> bool {
        self.end_address() == Ok(block.start_address())
    }

    /// Проверяет, что заданный `block` не пересекается с текущим.
    pub fn is_disjoint(
        &self,
        block: Self,
    ) -> bool {
        self.is_empty() ||
            block.is_empty() ||
            (!self.contains_address(block.start_address()) &&
                !block.contains_address(self.start_address()))
    }

    /// Возвращает `true`, если блок пуст.
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Сливает вместе два блока, если `block` смежный с текущим и лежит правее.
    ///
    /// Возвращает ошибку
    ///   - [`Error::InvalidArgument`] если блок `block` не смежный с текущим или не лежит правее.
    ///
    /// См. также [`Block::is_adjacent()`].
    pub fn coalesce(
        &self,
        block: Self,
    ) -> Result<Self> {
        if self.is_adjacent(block) {
            Ok(Block::from_index(self.start(), block.end())
                .expect("coalesce of valid blocks should be valid"))
        } else {
            Err(InvalidArgument)
        }
    }

    /// Возвращает пересечение блока `block` с текущим.
    pub fn intersection(
        &self,
        block: Self,
    ) -> Self {
        let start = cmp::max(self.start(), block.start());
        let end = cmp::min(self.end(), block.end());

        if start <= end {
            Block::from_index(start, end)
                .expect("an intersection of valid blocks should be a valid block")
        } else {
            Block::zero()
        }
    }

    /// Возвращает подблок исходного блока, задающийся диапазоном `range`.
    pub fn slice(
        &self,
        range: Range<usize>,
    ) -> Option<Self> {
        if range.start <= range.end && range.end <= self.count() {
            Self::from_index(self.start + range.start, self.start + range.end).ok()
        } else {
            None
        }
    }

    /// Разделяет блок на две дизъюнктные части:
    ///   - изменённый `self`;
    ///   - новый блок размером `count` единиц, взятый с конца текущего блока.
    ///
    /// Если в блоке недостаточно элементов, не меняя его возвращает `None`.
    pub fn tail(
        &mut self,
        count: usize,
    ) -> Option<Self> {
        if count <= self.count() {
            let new_start = self.end - count;
            let new_block = Block::from_index(new_start, self.end).ok()?;
            self.end = new_start;
            Some(new_block)
        } else {
            None
        }
    }
}

impl<T: Tag> Block<Addr<T>> {
    /// Для заданного блока виртуальных или физических адресов
    /// возвращает минимальный содержащий его
    /// блок виртуальных страниц [`Page`] или физических фреймов [`Frame`] соответственно.
    pub fn enclosing(&self) -> Block<Frage<T, L0_SIZE>> {
        let start_frage = Frage::<T, L0_SIZE>::index_by_address(self.start_address());
        let end_frage = if self.start == self.end {
            start_frage
        } else {
            Frage::<T, L0_SIZE>::index_by_address((self.end_address().unwrap() - 1).unwrap()) + 1
        };

        Block::from_index(start_frage, end_frage).unwrap()
    }
}

impl<T: Memory> Block<T>
where
    T::Address: IsVirt,
{
    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в указатель на константный [`u8`].
    pub fn into_ptr_u8(self) -> *const u8 {
        self.start_address().into_ptr_u8()
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в указатель на изменяемый [`u8`].
    pub fn into_mut_ptr_u8(self) -> *mut u8 {
        self.start_address().into_mut_ptr_u8()
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в указатель на константный `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    pub fn try_into_ptr<Q>(self) -> Result<*const Q> {
        if self.size() == mem::size_of::<Q>() {
            self.start_address().try_into_ptr::<Q>()
        } else {
            Err(InvalidArgument)
        }
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в указатель на изменяемый `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    pub fn try_into_mut_ptr<Q>(self) -> Result<*mut Q> {
        if self.size() == mem::size_of::<Q>() {
            self.start_address().try_into_mut_ptr::<Q>()
        } else {
            Err(InvalidArgument)
        }
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в неизменяемую ссылку на `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес блока нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_ref<'a, Q>(self) -> Result<&'a Q> {
        if self.size() == mem::size_of::<Q>() {
            let start_address = self.start_address();
            unsafe { start_address.try_into_ref::<Q>() }
        } else {
            Err(InvalidArgument)
        }
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в изменяемую ссылку на `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес блока нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_mut<'a, Q>(self) -> Result<&'a mut Q> {
        if self.size() == mem::size_of::<Q>() {
            let start_address = self.start_address();
            unsafe { start_address.try_into_mut::<Q>() }
        } else {
            Err(InvalidArgument)
        }
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в срез неизменяемых элементов типа `Q`.
    /// Размер среза вычисляется из размера блока и размера типа `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес блока нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_slice<'a, Q>(self) -> Result<&'a [Q]> {
        if self.size().is_multiple_of(mem::size_of::<Q>()) {
            let start_address = self.start_address();
            let len = self.size() / mem::size_of::<Q>();
            unsafe { start_address.try_into_slice::<Q>(len) }
        } else {
            Err(InvalidArgument)
        }
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в срез изменяемых элементов типа `Q`.
    /// Размер среза вычисляется из размера блока и размера типа `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес блока нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_mut_slice<'a, Q>(self) -> Result<&'a mut [Q]> {
        if self.size().is_multiple_of(mem::size_of::<Q>()) {
            let start_address = self.start_address();
            let len = self.size() / mem::size_of::<Q>();
            unsafe { start_address.try_into_mut_slice::<Q>(len) }
        } else {
            Err(InvalidArgument)
        }
    }

    /// Преобразует [`Block<Virt>`] или [`Block<Page>`] в [`NonNull<[Q]>`].
    /// Размер среза вычисляется из размера блока и размера типа `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если блок не соответствует типу `Q` по размеру.
    ///   - [`Error::InvalidAlignment`] если блок не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес блока нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_non_null_slice<Q>(self) -> Result<NonNull<[Q]>> {
        let slice = unsafe { self.try_into_mut_slice()? };
        Ok(slice.into())
    }
}

impl Block<Virt> {
    /// Преобразует указатель на `T` в [`Block<Virt>`].
    ///
    /// # Panics
    ///
    /// Паникует, если указатель не корректен.
    pub fn from_ptr<T>(x: *const T) -> Self {
        let start = Virt::from_ptr(x);
        let end = (start + mem::size_of::<T>()).unwrap();
        Self::new(start, end).unwrap()
    }

    /// Преобразует указатель на `T` в [`Block<Virt>`].
    ///
    /// # Panics
    ///
    /// Паникует, если указатель не корректен.
    pub fn from_mut_ptr<T>(x: *mut T) -> Self {
        Self::from_ptr(x)
    }

    /// Преобразует ссылку на `T` в [`Block<Virt>`].
    ///
    /// # Panics
    ///
    /// Паникует, если ссылка не корректна.
    pub fn from_ref<T>(x: &T) -> Self {
        Self::from_ptr(x)
    }

    /// Преобразует ссылку на `T` в [`Block<Virt>`].
    ///
    /// # Panics
    ///
    /// Паникует, если ссылка не корректна.
    pub fn from_mut<T>(x: &mut T) -> Self {
        Self::from_mut_ptr(x)
    }

    /// Преобразует срез элементов типа `T` в [`Block<Virt>`].
    ///
    /// # Panics
    ///
    /// Паникует, если срез не корректен.
    pub fn from_slice<T>(x: &[T]) -> Self {
        let range = x.as_ptr_range();

        Self::new(Virt::from_ptr(range.start), Virt::from_ptr(range.end)).unwrap()
    }

    /// Преобразует срез элементов типа `T` в [`Block<Virt>`].
    ///
    /// # Panics
    ///
    /// Паникует, если срез не корректен.
    pub fn from_slice_mut<T>(x: &mut [T]) -> Self {
        let range = x.as_ptr_range();

        Self::new(Virt::from_ptr(range.start), Virt::from_ptr(range.end)).unwrap()
    }
}

impl<T: Tag, const SIZE: usize> From<Block<Frage<T, SIZE>>> for Block<Addr<T>> {
    /// Преобразует блок виртуальных страниц или физических фреймов
    /// в блок виртуальных или физических адресов соответственно.
    fn from(block: Block<Frage<T, SIZE>>) -> Self {
        let start = block.start_address();
        let end = block.end_address().unwrap();
        Self::new(start, end).unwrap()
    }
}

impl<T: Memory> fmt::Debug for Block<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}, {}", self, T::NAME)?;

        if T::SIZE_OF != 1 && T::SIZE_OF != L0_SIZE {
            write!(formatter, "<{}>", Size::bytes(T::SIZE_OF))?;
        }

        write!(
            formatter,
            " count {}, [~{}, ",
            self.count(),
            Size::new::<T>(self.start),
        )?;

        if let Ok(end_address) = usize::try_from(self.end_address_u128()) {
            write!(formatter, "~{})", Size::bytes(end_address))
        } else {
            write!(formatter, "~16.000EiB)")
        }
    }
}

impl<T: Memory> fmt::Display for Block<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "[{}, {}{}), size {}",
            self.start_address(),
            T::HEX_PREFIX,
            GroupedHex::new(self.end_address_u128()),
            Size::new::<T>(self.count()),
        )
    }
}

impl<T: Memory> IntoIterator for Block<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.start .. self.end, PhantomData)
    }
}

impl<T: Memory> PartialOrd for Block<T> {
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        if self.end <= other.start {
            Some(Ordering::Less)
        } else if other.end <= self.start {
            Some(Ordering::Greater)
        } else if self.eq(other) {
            Some(Ordering::Equal)
        } else {
            None
        }
    }
}

/// Итератор по блоку.
///
/// В качестве итератора нельзя использовать сам [`Block`], так как он реализует типаж [`Copy`].
/// А делать итератор копируемым чревато ошибками.
#[derive(Clone)]
pub struct IntoIter<T: Memory>(Range<usize>, PhantomData<T>);

impl<T: Memory> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().and_then(|index| T::from_index(index).ok())
    }
}

/// Описывает базовые типы для [`Block`] и их операции.
pub trait Memory: Clone + Copy + Eq + PartialEq + SizeOf {
    /// Количество используемых битов в адресе.
    const ADDRESS_BITS: u32;

    /// Заменяет префикс `0x` при печати на:
    ///   - `0p` (**p**hysical) для физических адресов;
    ///   - `0v` (**v**irtual) для виртуальных адресов.
    const HEX_PREFIX: &'static str;

    /// Имя базового типа для отладочной печати блока.
    const NAME: &'static str;

    /// Тип адреса для базового типа:
    ///   - [`Virt`] для [`Virt`] и [`Page`];
    ///   - [`Phys`] для [`Phys`] и [`Frame`].
    type Address: fmt::Display + Ord + Clone + Copy;

    /// Возвращает адрес базового элемента `element`:
    ///   - `element` для `Addr`;
    ///   - `element.address()` для `Frage`.
    fn address(element: Self) -> Self::Address;

    /// Возвращает адрес базового элемента с индексом `index`:
    ///   - `Addr::new(index)` для `Addr`;
    ///   - `Frage::address_by_index(index)` для `Frage`.
    fn address_by_index(index: usize) -> Result<Self::Address>;

    /// Возвращает адрес базового элемента с индексом `index`:
    ///   - `Addr::new(index)` для `Addr`;
    ///   - `Frage::address_by_index(index)` для `Frage`.
    ///
    /// # Safety
    ///
    /// Параметр `address` должен задавать валидное значение адреса [`Self::Address`].
    unsafe fn address_by_index_unchecked(index: usize) -> Self::Address;

    /// Возвращает битовое представление адреса.
    fn address_into_usize(address: Self::Address) -> usize;

    /// Возвращает базовый элемент по его индексу `index`:
    ///   - `Addr::new(index)` для `Addr`;
    ///   - `Frage::from_index(index)` для `Frage`.
    fn from_index(index: usize) -> Result<Self>;

    /// Возвращает индекс базового элемента `element`:
    ///   - `element.into_usize()` для `Addr`;
    ///   - `element.index()` для `Frage`.
    fn index(element: Self) -> usize;

    /// Возвращает `true` если:
    ///   - Адреса `x` и `y` виртуальные и находятся в одной и той же
    ///     [половине виртуального адресного пространства](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details).
    ///   - Адреса `x` и `y` физические.
    fn is_same_half(
        x: Self::Address,
        y: Self::Address,
    ) -> bool;

    /// Возвращает адрес по его битовому представлению `address`.
    fn usize_into_address(address: usize) -> Result<Self::Address>;

    /// Возвращает адрес по его битовому представлению `address`.
    ///
    /// # Safety
    ///
    /// Параметр `address` должен задавать валидное значение адреса [`Self::Address`].
    unsafe fn usize_into_address_unchecked(address: usize) -> Self::Address;
}

impl<T: Tag> Memory for Addr<T> {
    const ADDRESS_BITS: u32 = T::BITS;
    const HEX_PREFIX: &'static str = T::HEX_PREFIX;
    const NAME: &'static str = T::ADDR_NAME;

    type Address = Addr<T>;

    fn address(element: Self) -> Self::Address {
        element
    }

    fn address_by_index(index: usize) -> Result<Self::Address> {
        Self::new(index)
    }

    unsafe fn address_by_index_unchecked(index: usize) -> Self::Address {
        unsafe { Self::new_unchecked(index) }
    }

    fn address_into_usize(address: Self::Address) -> usize {
        address.into_usize()
    }

    fn from_index(index: usize) -> Result<Self> {
        Self::new(index)
    }

    fn index(element: Self) -> usize {
        element.into_usize()
    }

    fn is_same_half(
        x: Self::Address,
        y: Self::Address,
    ) -> bool {
        T::is_same_half(x, y)
    }

    fn usize_into_address(address: usize) -> Result<Self::Address> {
        Self::Address::new(address)
    }

    unsafe fn usize_into_address_unchecked(address: usize) -> Self::Address {
        unsafe { Self::Address::new_unchecked(address) }
    }
}

impl<T: Tag, const SIZE: usize> Memory for Frage<T, SIZE> {
    const ADDRESS_BITS: u32 = T::BITS;
    const HEX_PREFIX: &'static str = T::HEX_PREFIX;
    const NAME: &'static str = T::FRAGE_NAME;

    type Address = Addr<T>;

    fn address(element: Self) -> Self::Address {
        element.address()
    }

    fn address_by_index(index: usize) -> Result<Self::Address> {
        Self::address_by_index(index)
    }

    unsafe fn address_by_index_unchecked(index: usize) -> Self::Address {
        unsafe { Self::from_index_unchecked(index).address() }
    }

    fn address_into_usize(address: Self::Address) -> usize {
        address.into_usize()
    }

    fn from_index(index: usize) -> Result<Self> {
        Self::from_index(index)
    }

    fn index(element: Self) -> usize {
        element.index()
    }

    fn is_same_half(
        x: Self::Address,
        y: Self::Address,
    ) -> bool {
        T::is_same_half(x, y)
    }

    fn usize_into_address(address: usize) -> Result<Self::Address> {
        Self::Address::new(address)
    }

    unsafe fn usize_into_address_unchecked(address: usize) -> Self::Address {
        unsafe { Self::Address::new_unchecked(address) }
    }
}

#[cfg(test)]
mod test {
    use crate::error::Error::{
        InvalidArgument,
        Overflow,
    };

    use super::{
        super::{
            Page,
            Phys,
            Virt,
        },
        Block,
    };

    #[test]
    fn grow_forward() {
        assert!(Block::<Phys>::from_index(0, 1).is_ok());
        assert!(Block::<Phys>::from_index(1, 1).is_ok());
        assert_eq!(Block::<Phys>::from_index(1, 0), Err(InvalidArgument));
    }

    #[test]
    fn compare() {
        let a = Block::from_index(0, 1).unwrap();
        let b = Block::from_index(1, 2).unwrap();
        let c = Block::from_index(1, 2).unwrap();
        let d = Block::from_index(2, 3).unwrap();

        let e = Block::<Virt>::from_index(0, 3).unwrap();

        assert_ne!(a, b);
        assert!(a < b);
        assert!(a <= b);
        assert!(b <= c);
        assert_eq!(b, c);
        assert!(c <= d);
        assert!(c < d);
        assert_ne!(c, d);

        for block in [a, b, c, d] {
            assert_ne!(block, e);
            assert_eq!(block.partial_cmp(&e), None);
            assert_eq!(e.partial_cmp(&block), None);
        }
    }

    #[test]
    fn full_halves() {
        let lower_half_first_page = LOWER_HALF_FIRST / Page::SIZE;
        let lower_half_last_page = LOWER_HALF_LAST / Page::SIZE;
        let higher_half_first_page = HIGHER_HALF_FIRST / Page::SIZE;
        let higher_half_last_page = HIGHER_HALF_LAST / Page::SIZE;

        let half_size = LOWER_HALF_LAST - LOWER_HALF_FIRST + 1;
        assert_eq!(half_size, HIGHER_HALF_LAST - HIGHER_HALF_FIRST + 1);

        // This way it works.
        let full_lower_half =
            Block::<Page>::from_index(lower_half_first_page, lower_half_last_page + 1).unwrap();
        assert!(full_lower_half.contains_address(Virt::new(LOWER_HALF_FIRST).unwrap()));
        assert!(full_lower_half.contains_address(Virt::new(LOWER_HALF_LAST).unwrap()));
        assert_eq!(full_lower_half.size(), half_size);
        assert_eq!(full_lower_half.end_address(), Err(InvalidArgument));
        assert_eq!(
            Block::<Page>::from_index(lower_half_first_page, lower_half_last_page + 2),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::<Page>::from_index(
                lower_half_first_page.wrapping_sub(1),
                lower_half_last_page + 1,
            ),
            Err(Overflow),
        );

        // But this way it should not:
        // ```rust
        // Block::new(
        //     Page::from_index(lower_half_first_page).unwrap(),
        //     Page::from_index(lower_half_last_page + 1).unwrap(),
        // )
        // ```
        // Because `Page::from_index(lower_half_last_page + 1)` has an invalid virtual address:
        assert_eq!(
            Page::from_index(lower_half_last_page + 1),
            Err(InvalidArgument),
        );

        let full_higher_half =
            Block::<Page>::from_index(higher_half_first_page, higher_half_last_page + 1).unwrap();
        assert!(full_higher_half.contains_address(Virt::new(HIGHER_HALF_LAST).unwrap()));
        assert_eq!(full_higher_half.end_address(), Err(Overflow));
        assert_eq!(full_higher_half.size(), half_size);
        assert_eq!(
            Block::<Page>::from_index(higher_half_first_page, higher_half_last_page + 2),
            Err(Overflow),
        );
        assert_eq!(
            Block::<Page>::from_index(higher_half_first_page - 1, higher_half_last_page + 1),
            Err(InvalidArgument),
        );

        let full_lower_half =
            Block::<Virt>::from_index(LOWER_HALF_FIRST, LOWER_HALF_LAST + 1).unwrap();
        assert!(full_lower_half.contains_address(Virt::new(LOWER_HALF_FIRST).unwrap()));
        assert!(full_lower_half.contains_address(Virt::new(LOWER_HALF_LAST).unwrap()));
        assert_eq!(full_lower_half.size(), half_size);
        assert_eq!(full_lower_half.end_address(), Err(InvalidArgument));
        assert_eq!(
            Block::<Virt>::from_index(LOWER_HALF_FIRST, LOWER_HALF_LAST + 2),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::<Virt>::from_index(LOWER_HALF_FIRST.wrapping_sub(1), LOWER_HALF_LAST + 1),
            Err(InvalidArgument),
        );

        // `HIGHER_HALF_LAST + 1` will overflow so
        // full higher half `Block<Virt>` is impossible to create and store.
        let full_higher_half =
            Block::<Virt>::from_index(HIGHER_HALF_FIRST, HIGHER_HALF_LAST.wrapping_add(1));
        assert_eq!(full_higher_half, Err(InvalidArgument));
        assert_eq!(
            Block::<Virt>::from_index(HIGHER_HALF_FIRST, HIGHER_HALF_LAST.wrapping_add(2)),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::<Virt>::from_index(HIGHER_HALF_FIRST - 1, HIGHER_HALF_LAST.wrapping_add(1)),
            Err(InvalidArgument),
        );
    }

    #[test]
    fn enforce_same_virt_half() {
        assert!(Block::<Virt>::from_index(0, LOWER_HALF_LAST).is_ok());
        assert!(Block::<Virt>::from_index(0, LOWER_HALF_LAST + 1).is_ok());

        let inside_lower_half = Virt::new(INSIDE_LOWER_HALF).unwrap();
        let inside_higher_half = Virt::new(INSIDE_HIGHER_HALF).unwrap();
        assert_eq!(
            Block::new(inside_lower_half, inside_higher_half),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::<Virt>::from_index(INSIDE_LOWER_HALF, INSIDE_HIGHER_HALF),
            Err(InvalidArgument),
        );

        let lower_half_last =
            Page::new(Virt::new(LOWER_HALF_LAST - (Page::SIZE - 1)).unwrap()).unwrap();
        let inside_lower_half = Page::new(inside_lower_half).unwrap();
        let inside_higher_half = Page::new(inside_higher_half).unwrap();
        assert!(Block::<Page>::from_index(0, lower_half_last.index()).is_ok());
        assert!(Block::<Page>::from_index(0, lower_half_last.index() + 1).is_ok());
        assert_eq!(
            Block::<Page>::from_index(0, lower_half_last.index() + 2),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::new(inside_lower_half, inside_higher_half),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::<Page>::from_index(inside_lower_half.index(), inside_higher_half.index()),
            Err(InvalidArgument),
        );
    }

    #[test]
    fn enclosing() {
        for base in [
            LOWER_HALF_FIRST,
            HIGHER_HALF_FIRST,
            INSIDE_LOWER_HALF,
            INSIDE_HIGHER_HALF,
        ] {
            for shift in [0, 1] {
                let start_virt = base + shift;
                let start_page = start_virt / Page::SIZE;
                for (end_virt, end_page) in [
                    (start_virt, start_page),
                    (start_virt + 1, start_page + 1),
                    (start_virt + (Page::SIZE - shift) - 1, start_page + 1),
                    (start_virt + (Page::SIZE - shift), start_page + 1),
                    (start_virt + (Page::SIZE - shift) + 1, start_page + 2),
                ] {
                    assert!(
                        Block::<Virt>::from_index(start_virt, end_virt).unwrap().enclosing() ==
                            Block::<Page>::from_index(start_page, end_page).unwrap(),
                    );
                }
            }
        }

        for base in [
            LOWER_HALF_FIRST + 2 * Page::SIZE - 1,
            LOWER_HALF_LAST,
            HIGHER_HALF_FIRST + 2 * Page::SIZE - 1,
            HIGHER_HALF_LAST,
        ] {
            for shift in [0, 1] {
                let end_virt = base - shift;
                let end_page = end_virt / Page::SIZE + 1;
                for (start_virt, start_page) in [
                    (end_virt - 1, end_page - 1),
                    (end_virt - (Page::SIZE - shift) + 1, end_page - 1),
                    (end_virt - (Page::SIZE - shift), end_page - 2),
                    (end_virt - (Page::SIZE - shift) - 1, end_page - 2),
                ] {
                    assert_eq!(
                        Block::<Virt>::from_index(start_virt, end_virt).unwrap().enclosing(),
                        Block::<Page>::from_index(start_page, end_page).unwrap(),
                    );
                }
                assert_eq!(
                    Block::<Virt>::from_index(end_virt, end_virt).unwrap().enclosing(),
                    Block::<Page>::from_index(end_page - 1, end_page - 1).unwrap(),
                );
            }
        }
    }

    #[test]
    fn bad_address() {
        let phys_end = 1 << 52;
        assert!(Block::<Phys>::from_index(phys_end - 1, phys_end).is_ok());
        assert_eq!(
            Block::<Phys>::from_index(phys_end, phys_end),
            Err(InvalidArgument),
        );
        assert_eq!(
            Block::<Phys>::from_index(phys_end - 1, phys_end + 1),
            Err(InvalidArgument),
        );

        let bad_virt = 1 << 48;
        assert_eq!(
            Block::<Virt>::from_index(bad_virt, bad_virt),
            Err(InvalidArgument),
        );
    }

    #[test]
    #[cfg_attr(debug_assertions, should_panic)]
    fn panic_on_local_variables() {
        let a = 1;
        let _ = Block::from_ref(&a);
    }

    const LOWER_HALF_FIRST: usize = 0x0;
    const LOWER_HALF_LAST: usize = 0x0000_7FFF_FFFF_FFFF;
    const HIGHER_HALF_FIRST: usize = 0xFFFF_8000_0000_0000;
    const HIGHER_HALF_LAST: usize = 0xFFFF_FFFF_FFFF_FFFF;
    const INSIDE_LOWER_HALF: usize = (LOWER_HALF_LAST / (2 * Page::SIZE)) * Page::SIZE;
    const INSIDE_HIGHER_HALF: usize = 0xFFFF_FFFF_0000_0000;
}
