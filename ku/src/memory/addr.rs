use core::{
    cmp::PartialEq,
    fmt,
    marker::PhantomData,
    mem,
    ops::{
        Add,
        BitAnd,
        Shl,
        Shr,
        Sub,
    },
    slice,
};

use num_traits::{
    One,
    Zero,
};
use serde::{
    Deserialize,
    Serialize,
};
use static_assertions::{
    const_assert,
    const_assert_eq,
};
use x86_64::addr::VirtAddr;

use crate::{
    backtrace,
    error::{
        Error::{
            InvalidAlignment,
            InvalidArgument,
            Null,
            Overflow,
        },
        Result,
    },
};

use super::{
    mmu::{
        PAGE_OFFSET_BITS,
        PAGE_TABLE_ENTRY_COUNT,
        PAGE_TABLE_INDEX_BITS,
        PAGE_TABLE_INDEX_MASK,
        PAGE_TABLE_LEAF_LEVEL,
        PAGE_TABLE_LEVEL_COUNT,
        PAGE_TABLE_ROOT_LEVEL,
    },
    size::{
        self,
        SizeOf,
    },
};

// Used in docs.
#[allow(unused)]
use crate::{
    self as ku,
    error::Error,
    memory::Port,
};

/// Базовый тип для адресов [архитектуры x86-64](https://wiki.osdev.org/X86-64), как виртуальных, так и физических.
#[derive(Clone, Copy, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(transparent)]
pub struct Addr<T>(pub(super) usize, pub(super) PhantomData<T>);

const_assert_eq!(mem::size_of::<Addr<()>>(), mem::size_of::<usize>());
const_assert_eq!(mem::size_of::<Addr<()>>(), mem::size_of::<u64>());

impl<T: Tag> Addr<T> {
    /// Создаёт [`Addr`] --- [`Phys`] или [`Virt`] --- по его битовому представлению `addr`.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`] если битовое представление `addr`
    /// не является корректным для адреса целевого типа.
    pub fn new(addr: usize) -> Result<Self> {
        T::new(addr)
    }

    /// Создаёт [`Addr`] --- [`Phys`] или [`Virt`] --- по его битовому представлению `addr`.
    ///
    /// # Safety
    ///
    /// Битовое представление `addr` должно являться корректным для адреса целевого типа.
    pub unsafe fn new_unchecked(addr: usize) -> Self {
        unsafe { T::new_unchecked(addr) }
    }

    /// Создаёт [`Addr`] --- [`Phys`] или [`Virt`] --- по его битовому представлению `addr`.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`] если битовое представление `addr`
    /// не является корректным для адреса целевого типа.
    pub fn new_u64(addr: u64) -> Result<Self> {
        Self::new(size::from(addr))
    }

    /// Возвращает нулевой [`Addr`] --- [`Phys`] или [`Virt`].
    /// В отличие от [`Addr::default()`] доступна в константном контексте,
    /// поэтому не может использовать `T::new()`.
    pub const fn zero() -> Self {
        Self(0, PhantomData)
    }

    /// Возвращает битовое представление адреса.
    pub fn into_usize(self) -> usize {
        self.0
    }

    /// Возвращает битовое представление адреса.
    pub fn into_u64(self) -> u64 {
        size::into_u64(self.0)
    }

    /// Возвращает битовое представление адреса в виде целого типа,
    /// для которого есть [`TryFrom<u64>`], например в виде [`u32`].
    /// Возвращает ошибку [`Error::Int`], если адрес не помещается в выбранный тип.
    pub fn try_into<Q: TryFrom<u64>>(self) -> Result<Q>
    where
        Error: From<<Q as TryFrom<u64>>::Error>,
    {
        size::try_into(self.0)
    }
}

impl<T: Tag> Add<usize> for Addr<T> {
    type Output = Result<Self>;

    fn add(
        self,
        rhs: usize,
    ) -> Self::Output {
        self.0.checked_add(rhs).ok_or(Overflow).and_then(Self::new).and_then(|addr| {
            if T::is_same_half(self, addr) {
                Ok(addr)
            } else {
                Err(Overflow)
            }
        })
    }
}

impl<T: Tag> Sub<usize> for Addr<T> {
    type Output = Result<Self>;

    fn sub(
        self,
        rhs: usize,
    ) -> Self::Output {
        self.0.checked_sub(rhs).ok_or(Overflow).and_then(Self::new).and_then(|addr| {
            if T::is_same_half(self, addr) {
                Ok(addr)
            } else {
                Err(Overflow)
            }
        })
    }
}

impl<T: Tag> Sub<Self> for Addr<T> {
    type Output = Result<usize>;

    fn sub(
        self,
        rhs: Self,
    ) -> Self::Output {
        if T::is_same_half(self, rhs) {
            self.0.checked_sub(rhs.0).ok_or(Overflow)
        } else {
            Err(Overflow)
        }
    }
}

impl<T: Tag> fmt::Debug for Addr<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}({}{:X})", T::ADDR_NAME, T::HEX_PREFIX, self.0)
    }
}

impl<T: Tag> fmt::Display for Addr<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}{}", T::HEX_PREFIX, GroupedHex(self.0))
    }
}

/// Требования к типам, которые можно печатать в шестнадцатеричном виде с группировкой по разрядам.
pub trait GroupedHexTrait<T> = BitAnd<Output = T>
    + Copy
    + One
    + PartialEq
    + Shl<u32, Output = T>
    + Shr<u32, Output = T>
    + Sub<Output = T>
    + Zero
    + fmt::UpperHex;

/// Вспомогательная обёртка для печати шестнадцатеричных чисел с группировкой по разрядам.
pub struct GroupedHex<T: GroupedHexTrait<T>, const GROUP_HEX_DIGITS: usize = 4>(T);

impl<T: GroupedHexTrait<T>> GroupedHex<T> {
    /// Возвращает обёртку для печати шестнадцатеричных чисел с группировкой по разрядам.
    pub fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T: GroupedHexTrait<T>> GroupedHex<T> {
    /// Количество бит в одной шестнадцатеричной цифре.
    const BITS_PER_HEX_DIGIT: usize = 4;

    /// Количество шестнадцатеричных цифр в одной группе, отделённой от других символом `_`.
    const GROUP_HEX_DIGITS: usize = 4;

    /// Количество бит в одной группе шестнадцатеричных цифр.
    const GROUP_BITS: u32 = (Self::GROUP_HEX_DIGITS * Self::BITS_PER_HEX_DIGIT) as u32;
}

impl<T: GroupedHexTrait<T>> fmt::Display for GroupedHex<T> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        let group_mask = (T::one() << Self::GROUP_BITS) - T::one();

        let mut groups = (0 .. usize::BITS / Self::GROUP_BITS)
            .rev()
            .map(|index| (index, (self.0 >> (index * Self::GROUP_BITS)) & group_mask))
            .skip_while(|&(index, group)| index > 0 && group == T::zero())
            .map(|(_, group)| group);

        write!(
            formatter,
            "{:X}",
            groups.next().expect("there should be at least one group of digits"),
        )?;

        for group in groups {
            write!(
                formatter,
                "_{:0width$X}",
                group,
                width = Self::GROUP_HEX_DIGITS,
            )?;
        }

        Ok(())
    }
}

/// Тег, позволяющий различать
/// виртуальные адреса [`ku::memory::Virt`] и физические [`ku::memory::Phys`],
/// а также (виртуальные) страницы [`ku::memory::Page`] и (физические) фреймы [`ku::memory::Frame`].
pub trait Tag: Clone + Copy + Default + Eq + Ord {
    /// Создаёт [`Addr`] --- [`Phys`], [`Virt`] или [`Port`] ---
    /// по его битовому представлению `addr`.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`] если битовое представление `addr`
    /// не является корректным для адреса целевого типа.
    fn new(addr: usize) -> Result<Addr<Self>>;

    /// Создаёт [`Addr`] --- [`Phys`], [`Virt`] или [`Port`] ---
    /// по его битовому представлению `addr`.
    ///
    /// # Safety
    ///
    /// Битовое представление `addr` должно являться корректным для адреса целевого типа.
    unsafe fn new_unchecked(addr: usize) -> Addr<Self>;

    /// Возвращает `true` если:
    ///   - Адреса `x` и `y` виртуальные и находятся в одной и той же
    ///     [половине виртуального адресного пространства](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details).
    ///   - Адреса `x` и `y` физические.
    ///   - Адреса `x` и `y` задают номера
    ///     [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    fn is_same_half(
        x: Addr<Self>,
        y: Addr<Self>,
    ) -> bool;

    /// Задаёт имя типа при печати:
    ///   - `"Phys"` для физических адресов;
    ///   - `"Virt"` для виртуальных адресов;
    ///   - `"Port"` для
    ///     [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    const ADDR_NAME: &'static str;

    /// Количество используемых битов в адресе.
    const BITS: u32;

    /// Задаёт имя типа при печати:
    ///   - `"Frame"` для физических фреймов;
    ///   - `"Page"` для виртуальных страниц;
    ///   - `"Port"` для
    ///     [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    const FRAGE_NAME: &'static str;

    /// Заменяет префикс `0x` при печати на:
    ///   - `0p` (**p**hysical) для физических адресов;
    ///   - `0v` (**v**irtual) для виртуальных адресов;
    ///   - `0x` для номеров
    ///     [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    const HEX_PREFIX: &'static str;
}

/// Тег физических адресов [`ku::memory::Phys`] и фреймов [`ku::memory::Frame`].
#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct PhysTag;

impl Tag for PhysTag {
    fn new(addr: usize) -> Result<Phys> {
        Phys::new_impl(addr)
    }

    unsafe fn new_unchecked(addr: usize) -> Phys {
        unsafe { Phys::new_unchecked_impl(addr) }
    }

    fn is_same_half(
        _x: Phys,
        _y: Phys,
    ) -> bool {
        true
    }

    const ADDR_NAME: &'static str = "Phys";
    const BITS: u32 = Phys::BITS;
    const FRAGE_NAME: &'static str = "Frame";
    const HEX_PREFIX: &'static str = "0p";
}

/// Тег виртуальных адресов и страниц.
#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct VirtTag;

impl Tag for VirtTag {
    fn new(addr: usize) -> Result<Virt> {
        Virt::new_impl(addr)
    }

    unsafe fn new_unchecked(addr: usize) -> Virt {
        unsafe { Virt::new_unchecked_impl(addr) }
    }

    fn is_same_half(
        x: Virt,
        y: Virt,
    ) -> bool {
        ((x.into_usize() ^ y.into_usize()) >> (Addr::<Self>::BITS - 1)) == 0
    }

    const ADDR_NAME: &'static str = "Virt";
    const BITS: u32 = Virt::BITS;
    const FRAGE_NAME: &'static str = "Page";
    const HEX_PREFIX: &'static str = "0v";
}

/// Используется для [`ku::memory::Block<Virt>`] и [`ku::memory::Block<Page>`].
#[doc(hidden)]
pub trait IsVirt {
    fn from_ptr<T: ?Sized>(ptr: *const T) -> Self;
    fn from_mut_ptr<T: ?Sized>(ptr: *mut T) -> Self;
    fn from_ref<T: ?Sized>(x: &T) -> Self;
    fn from_mut<T: ?Sized>(x: &mut T) -> Self;
    fn into_ptr_u8(self) -> *const u8;
    fn into_mut_ptr_u8(self) -> *mut u8;
    fn try_into_ptr<T>(self) -> Result<*const T>;
    fn try_into_mut_ptr<T>(self) -> Result<*mut T>;
    unsafe fn try_into_ref<'a, T>(self) -> Result<&'a T>;
    unsafe fn try_into_mut<'a, T>(self) -> Result<&'a mut T>;
    unsafe fn try_into_slice<'a, T>(
        self,
        len: usize,
    ) -> Result<&'a [T]>;
    unsafe fn try_into_mut_slice<'a, T>(
        self,
        len: usize,
    ) -> Result<&'a mut [T]>;
}

/// Физический адрес [архитектуры x86-64](https://wiki.osdev.org/X86-64).
///
/// # Examples
///
/// ## Преобразования между `Phys` и базовыми типами
/// Преобразования между [`Phys`] и [`usize`](https://doc.rust-lang.org/core/primitive.usize.html)/[`u64`](https://doc.rust-lang.org/core/primitive.u64.html).
/// ```rust
/// # use ku::error::Result;
/// # use ku::memory::Phys;
/// #
/// # fn f() -> Result<()> {
/// let phys = Phys::new(0x123ABC)?;
/// assert_eq!(phys, Phys::new_u64(0x123ABC)?);
/// assert_eq!(phys.into_u64(), 0x123ABC_u64);
/// assert_eq!(phys.into_usize(), 0x123ABC_usize);
/// # Ok(())
/// # }
/// #
/// # assert!(f().is_ok());
/// ```
///
/// ## Некорректные адреса
/// [`Phys::new()`] возвращает ошибку [`ku::error::Error::InvalidArgument`]
/// при попытке задать некорректный с точки зрения [архитектуры x86-64](https://wiki.osdev.org/X86-64) физический адрес.
/// ```rust
/// # use ku::error::Error::InvalidArgument;
/// # use ku::memory::Phys;
/// #
/// let bad_address = 1 << 63;
/// assert_eq!(Phys::new(bad_address), Err(InvalidArgument));
/// ```
pub type Phys = Addr<PhysTag>;

/// Виртуальный адрес [архитектуры x86-64](https://wiki.osdev.org/X86-64).
///
/// # Examples
///
/// ## Преобразования между `Virt` и базовыми типами
/// Преобразования между [`Virt`] и
/// [`usize`](https://doc.rust-lang.org/core/primitive.usize.html)/[`u64`](https://doc.rust-lang.org/core/primitive.u64.html).
/// ```rust
/// # use ku::error::Result;
/// # use ku::memory::Virt;
/// #
/// # fn f() -> Result<()> {
/// let virt = Virt::new(0x123ABC)?;
/// assert_eq!(virt, Virt::new_u64(0x123ABC)?);
/// assert_eq!(virt.into_u64(), 0x123ABC_u64);
/// assert_eq!(virt.into_usize(), 0x123ABC_usize);
/// # Ok(())
/// # }
/// #
/// # assert!(f().is_ok());
/// ```
///
/// ## Некорректные адреса
/// [`Virt::new()`] возвращает ошибку [`ku::error::Error::InvalidArgument`]
/// при попытке задать некорректный с точки зрения
/// [архитектуры x86-64](https://wiki.osdev.org/X86-64) виртуальный адрес.
/// ```rust
/// # use ku::memory::Virt;
/// # use ku::error::Error::InvalidArgument;
/// #
/// let bad_address = 1 << 63;
/// assert_eq!(Virt::new(bad_address), Err(InvalidArgument));
/// ```
///
/// ## Преобразования между `Virt` и `VirtAddr`
/// [`Virt`] поддерживает преобразования в/из [`x86_64::addr::VirtAddr`].
/// Это нужно для использования структур из [`x86_64`],
/// например [`x86_64::structures::tss::TaskStateSegment`] и
/// [`x86_64::structures::idt::Entry::set_handler_addr()`].
/// ```rust
/// # use ku::error::Result;
/// # use ku::memory::Virt;
/// # use x86_64::VirtAddr;
/// #
/// fn f(virt_addr: VirtAddr) -> Virt {
///     virt_addr.into()
/// }
///
/// # fn g() -> Result<()> {
/// let virt = Virt::new(0x123ABC)?;
/// let virt_addr: VirtAddr = virt.into();
/// let virt2: Virt = virt_addr.into();
/// assert_eq!(virt, virt2);
/// assert_eq!(virt, f(virt.into()));
/// # Ok(())
/// # }
/// #
/// # assert!(g().is_ok());
/// ```
pub type Virt = Addr<VirtTag>;

impl Phys {
    /// Создаёт физический адрес по его битовому представлению `addr`.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`] если битовое представление `addr`
    /// не является корректным для физического адреса.
    fn new_impl(addr: usize) -> Result<Self> {
        let zeros = addr.leading_zeros();
        if zeros >= Self::UNUSED_BITS {
            Ok(unsafe { Self::new_unchecked_impl(addr) })
        } else {
            Err(InvalidArgument)
        }
    }

    /// Создаёт физический адрес по его битовому представлению `addr`.
    ///
    /// # Safety
    ///
    /// Битовое представление `addr` должно являться корректным для физического адреса.
    unsafe fn new_unchecked_impl(addr: usize) -> Self {
        Self(addr, PhantomData)
    }

    /// Создаёт физический адрес по его битовому представлению `addr`.
    ///
    /// Не может завершиться ошибкой,
    /// так как любое [`u32`] является корректным для физического адреса.
    pub fn new_u32(addr: u32) -> Self {
        const_assert!(u32::BITS <= Phys::BITS);

        unsafe { Self::new_unchecked_impl(size::from(addr)) }
    }

    /// Количество используемых битов в физическом адресе.
    pub const BITS: u32 = 52;

    /// Количество неиспользуемых битов в физическом адресе.
    const UNUSED_BITS: u32 = usize::BITS - Self::BITS;
}

impl Virt {
    /// Создаёт виртуальный адрес по его битовому представлению `addr`.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`] если битовое представление `addr`
    /// не является корректным для виртуального адреса.
    fn new_impl(addr: usize) -> Result<Self> {
        if addr.leading_zeros() > Self::UNUSED_BITS || addr.leading_ones() > Self::UNUSED_BITS {
            Ok(unsafe { Self::new_unchecked_impl(addr) })
        } else {
            Err(InvalidArgument)
        }
    }

    /// Создаёт виртуальный адрес по его битовому представлению `addr`.
    ///
    /// # Safety
    ///
    /// Битовое представление `addr` должно являться корректным для виртуального адреса.
    unsafe fn new_unchecked_impl(addr: usize) -> Self {
        Self(addr, PhantomData)
    }

    /// Создаёт виртуальный адрес по его битовому представлению `addr`.
    ///
    /// Если битовое представление `addr` не является корректным для виртуального адреса,
    /// [канонизирует](https://en.wikipedia.org/wiki/X86-64#Canonical_form_addresses)
    /// его.
    pub fn canonize(mut addr: usize) -> Self {
        if addr > !Self::NEGATIVE_SIGN_EXTEND / 2 {
            addr |= Self::NEGATIVE_SIGN_EXTEND;
        }
        unsafe { Self::new_unchecked_impl(addr) }
    }

    /// Размер в байтах каждой из
    /// [половин](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn half_size() -> usize {
        1 << (Self::BITS - 1)
    }

    /// Возвращает первый адрес
    /// [верхней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn higher_half() -> Self {
        Self::new_impl(!0 << (Self::BITS - 1)).expect("first valid address of the high area")
    }

    /// Возвращает первый адрес
    /// [нижней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn lower_half() -> Self {
        Self::default()
    }

    /// Возвращает индекс записи в узле таблицы страниц уровня `level`,
    /// соответствующей виртуальному адресу.
    pub fn page_table_index(
        &self,
        level: u32,
    ) -> usize {
        let shift = PAGE_OFFSET_BITS + level * PAGE_TABLE_INDEX_BITS;
        (self.into_usize() >> shift) & PAGE_TABLE_INDEX_MASK
    }

    /// Возвращает `true` если виртуальный адрес относится к
    /// [верхней половине](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn is_higher_half(&self) -> bool {
        !self.is_lower_half()
    }

    /// Возвращает `true` если виртуальный адрес относится к
    /// [нижней половине](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn is_lower_half(&self) -> bool {
        self.into_usize() >> Self::BITS == 0
    }

    /// Создаёт виртуальный адрес по его индексам `page_table_indexes` в узлах таблицы страниц,
    /// от корневого `PAGE_TABLE_LEAF_LEVEL` до листьевого `PAGE_TABLE_LEAF_LEVEL`,
    /// и смещения внутри страницы `offset`.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    /// - Хотя бы один из индексов в `page_table_indexes` превышает количество записей в
    ///   узле таблицы страниц `PAGE_TABLE_ENTRY_COUNT`.
    /// - Смещение `offset` превышает `1 << PAGE_OFFSET_BITS` --- 4 KiB.
    pub fn from_page_table_indexes(
        page_table_indexes: [usize; PAGE_TABLE_LEVEL_COUNT],
        offset: usize,
    ) -> Self {
        assert!(page_table_indexes.iter().all(|&index| index < PAGE_TABLE_ENTRY_COUNT));
        assert!(offset < 1 << PAGE_OFFSET_BITS);

        let mut virt = 0;
        for level in (PAGE_TABLE_LEAF_LEVEL ..= PAGE_TABLE_ROOT_LEVEL).rev() {
            virt = (virt << PAGE_TABLE_INDEX_BITS) | page_table_indexes[size::from(level)];
        }
        virt = (virt << PAGE_OFFSET_BITS) | offset;

        Virt::canonize(virt)
    }

    /// Преобразует указатель на `T` в [`Virt`].
    ///
    /// # Panics
    ///
    /// Паникует, если `ptr` похож на указатель на локальную переменную и
    /// включены `cfg!(debug_assertions)`.
    pub fn from_ptr<T: ?Sized>(ptr: *const T) -> Self {
        const_assert_eq!(mem::size_of::<*const ()>(), mem::size_of::<usize>());
        debug_assert!(
            !backtrace::is_local_variable(ptr),
            "attempted to take a Virt address of a local variable at {ptr:?} which probably does \
             not live long enough",
        );
        unsafe { Self::new_unchecked_impl(ptr as *const () as usize) }
    }

    /// Преобразует указатель на `T` в [`Virt`].
    ///
    /// # Panics
    ///
    /// Паникует, если указатель не корректен.
    pub fn from_mut_ptr<T: ?Sized>(ptr: *mut T) -> Self {
        Self::from_ptr(ptr)
    }

    /// Преобразует ссылку на `T` в [`Virt`].
    ///
    /// # Panics
    ///
    /// Паникует, если ссылка не корректна.
    pub fn from_ref<T: ?Sized>(x: &T) -> Self {
        Self::from_ptr(x)
    }

    /// Преобразует ссылку на `T` в [`Virt`].
    ///
    /// # Panics
    ///
    /// Паникует, если ссылка не корректна.
    pub fn from_mut<T: ?Sized>(x: &mut T) -> Self {
        Self::from_mut_ptr(x)
    }

    /// Преобразует [`Virt`] в указатель на константный [`u8`].
    pub fn into_ptr_u8(self) -> *const u8 {
        const_assert_eq!(mem::align_of::<u8>(), 1);
        const_assert_eq!(mem::size_of::<*const u8>(), mem::size_of::<usize>());
        self.0 as *const u8
    }

    /// Преобразует [`Virt`] в указатель на изменяемый [`u8`].
    pub fn into_mut_ptr_u8(self) -> *mut u8 {
        const_assert_eq!(mem::align_of::<u8>(), 1);
        const_assert_eq!(mem::size_of::<*mut u8>(), mem::size_of::<usize>());
        self.0 as *mut u8
    }

    /// Преобразует [`Virt`] в указатель на константный `T`.
    ///
    /// Возвращает ошибку [`Error::InvalidAlignment`],
    /// если адрес не соответствует типу `T` по выравниванию.
    pub fn try_into_ptr<T>(self) -> Result<*const T> {
        const_assert_eq!(mem::size_of::<*const ()>(), mem::size_of::<usize>());
        if self.0.is_multiple_of(mem::align_of::<T>()) {
            Ok(self.0 as *const T)
        } else {
            Err(InvalidAlignment)
        }
    }

    /// Преобразует [`Virt`] в указатель на изменяемый `T`.
    ///
    /// Возвращает ошибку [`Error::InvalidAlignment`],
    /// если адрес не соответствует типу `T` по выравниванию.
    pub fn try_into_mut_ptr<T>(self) -> Result<*mut T> {
        const_assert_eq!(mem::size_of::<*mut ()>(), mem::size_of::<usize>());
        if self.0.is_multiple_of(mem::align_of::<T>()) {
            Ok(self.0 as *mut T)
        } else {
            Err(InvalidAlignment)
        }
    }

    /// Преобразует [`Virt`] в неизменяемую ссылку на `T`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidAlignment`] если адрес не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_ref<'a, T>(self) -> Result<&'a T> {
        let ptr = self.try_into_ptr::<T>()?;
        if ptr.is_null() {
            Err(Null)
        } else {
            Ok(unsafe { &*ptr })
        }
    }

    /// Преобразует [`Virt`] в изменяемую ссылку на `T`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidAlignment`] если адрес не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_mut<'a, T>(self) -> Result<&'a mut T> {
        let ptr = self.try_into_mut_ptr::<T>()?;
        if ptr.is_null() {
            Err(Null)
        } else {
            Ok(unsafe { &mut *ptr })
        }
    }

    /// Преобразует [`Virt`] в срез из `len` неизменяемых элементов типа `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidAlignment`] если адрес не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_slice<'a, T>(
        self,
        len: usize,
    ) -> Result<&'a [T]> {
        let ptr = self.try_into_ptr::<T>()?;
        if ptr.is_null() {
            Err(Null)
        } else {
            Ok(unsafe { slice::from_raw_parts(ptr, len) })
        }
    }

    /// Преобразует [`Virt`] в срез из `len` изменяемых элементов типа `Q`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidAlignment`] если адрес не соответствует типу `Q` по выравниванию.
    ///   - [`Error::Null`] если адрес нулевой.
    ///
    /// # Safety
    ///
    /// Вызывающая сторона должна гарантировать,
    /// что не будут нарушены инварианты управления памятью в Rust,
    /// которые не относятся к возвращаемым ошибкам.
    pub unsafe fn try_into_mut_slice<'a, T>(
        self,
        len: usize,
    ) -> Result<&'a mut [T]> {
        let ptr = self.try_into_mut_ptr::<T>()?;
        if ptr.is_null() {
            Err(Null)
        } else {
            Ok(unsafe { slice::from_raw_parts_mut(ptr, len) })
        }
    }

    /// Количество используемых битов в физическом адресе.
    const BITS: u32 = PAGE_TABLE_INDEX_BITS * (PAGE_TABLE_ROOT_LEVEL + 1) + PAGE_OFFSET_BITS;

    /// Маска для
    /// [знакового расширения](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// битового представления виртуального адреса.
    const NEGATIVE_SIGN_EXTEND: usize = !0 << Self::BITS;

    /// Количество неиспользуемых битов в физическом адресе.
    const UNUSED_BITS: u32 = usize::BITS - Self::BITS;
}

impl IsVirt for Virt {
    fn from_ptr<T: ?Sized>(ptr: *const T) -> Self {
        Virt::from_ptr::<T>(ptr)
    }

    fn from_mut_ptr<T: ?Sized>(ptr: *mut T) -> Self {
        Virt::from_mut_ptr::<T>(ptr)
    }

    fn from_ref<T: ?Sized>(x: &T) -> Self {
        Virt::from_ref::<T>(x)
    }

    fn from_mut<T: ?Sized>(x: &mut T) -> Self {
        Virt::from_mut::<T>(x)
    }

    fn into_ptr_u8(self) -> *const u8 {
        Virt::into_ptr_u8(self)
    }

    fn into_mut_ptr_u8(self) -> *mut u8 {
        Virt::into_mut_ptr_u8(self)
    }

    fn try_into_ptr<T>(self) -> Result<*const T> {
        Virt::try_into_ptr::<T>(self)
    }

    fn try_into_mut_ptr<T>(self) -> Result<*mut T> {
        Virt::try_into_mut_ptr::<T>(self)
    }

    unsafe fn try_into_ref<'a, T>(self) -> Result<&'a T> {
        unsafe { Virt::try_into_ref::<'a, T>(self) }
    }

    unsafe fn try_into_mut<'a, T>(self) -> Result<&'a mut T> {
        unsafe { Virt::try_into_mut::<'a, T>(self) }
    }

    unsafe fn try_into_slice<'a, T>(
        self,
        len: usize,
    ) -> Result<&'a [T]> {
        unsafe { Virt::try_into_slice::<T>(self, len) }
    }

    unsafe fn try_into_mut_slice<'a, T>(
        self,
        len: usize,
    ) -> Result<&'a mut [T]> {
        unsafe { Virt::try_into_mut_slice::<T>(self, len) }
    }
}

impl From<VirtAddr> for Virt {
    fn from(addr: VirtAddr) -> Self {
        Self::new_u64(addr.as_u64()).expect("bad VirtAddr")
    }
}

impl From<Virt> for VirtAddr {
    fn from(addr: Virt) -> Self {
        VirtAddr::new(addr.into_u64())
    }
}

impl<T: Tag> SizeOf for Addr<T> {
    const SIZE_OF: usize = 1;
}

#[cfg(test)]
mod test {
    use super::{
        Tag,
        Virt,
        VirtTag,
    };

    #[test]
    fn two_separate_halves_of_virt() {
        let lower_half_first = Virt::new(0).unwrap();
        let lower_half_last = Virt::new(0x0000_7FFF_FFFF_FFFF).unwrap();
        let higher_half_first = Virt::new(0xFFFF_8000_0000_0000).unwrap();
        let higher_half_last = Virt::new(0xFFFF_FFFF_FFFF_FFFF).unwrap();

        assert!(VirtTag::is_same_half(lower_half_first, lower_half_last));
        assert!(VirtTag::is_same_half(higher_half_first, higher_half_last));
        assert!(!VirtTag::is_same_half(lower_half_first, higher_half_first));
        assert!(!VirtTag::is_same_half(lower_half_first, higher_half_last));
        assert!(!VirtTag::is_same_half(lower_half_last, higher_half_first));
        assert!(!VirtTag::is_same_half(lower_half_last, higher_half_last));

        assert!(
            (lower_half_last + (higher_half_first.into_usize() - lower_half_last.into_usize()))
                .is_err()
        );

        for shift in [0, 1, 2, 10, 20, 30, 40, 50, 63] {
            let mut delta = (1usize << shift).wrapping_sub(0x10);
            for _ in 0 ..= 0x20 {
                if delta != 0 {
                    assert!((lower_half_first - delta).is_err());
                    assert!((lower_half_last + delta).is_err());
                    assert!((higher_half_first - delta).is_err());
                    assert!((higher_half_last + delta).is_err());
                }
                delta = delta.wrapping_add(1);
            }
        }
    }

    #[test]
    #[cfg_attr(debug_assertions, should_panic)]
    fn panic_on_local_variables() {
        let a = 1;
        let _ = Virt::from_ref(&a);
    }
}
