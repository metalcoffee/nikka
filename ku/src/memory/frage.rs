use core::{
    alloc::Layout,
    fmt,
    iter::Step,
    ops::{
        Add,
        Range,
        Sub,
    },
};

use crate::error::{
    Error::{
        InvalidAlignment,
        Overflow,
    },
    Result,
};

use super::{
    addr::{
        Addr,
        PhysTag,
        Tag,
        VirtTag,
    },
    mmu::{
        PAGE_OFFSET_BITS,
        PAGE_TABLE_INDEX_BITS,
    },
    size::{
        Size,
        SizeOf,
    },
};

// Used in docs.
#[allow(unused)]
use {
    super::{
        Phys,
        Virt,
        mmu::{
            PAGE_TABLE_LEAF_LEVEL,
            PageTableEntry,
            PageTableFlags,
        },
    },
    crate::error::Error,
};

/// Обобщённый тип для (виртуальных) страниц памяти и (физических) фреймов.
#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Frage<T: Tag, const SIZE: usize>(Addr<T>);

impl<T: Tag, const SIZE: usize> Frage<T, SIZE> {
    /// Размер физического фрейма или виртуальной страницы.
    pub const SIZE: usize = SIZE;

    /// Количество физических фреймов [`L0Frame`] или виртуальных страниц [`L0Page`]
    /// стандартного с точки зрения MMU размера,
    /// помещающихся в этом физическом фрейме или виртуальной странице.
    pub const L0_COUNT: usize = Self::SIZE / L0Page::SIZE;

    /// Создаёт [`Frage`] --- [`Frame`] или [`Page`] ---
    /// по его начальному адресу `addr` --- [`Phys`] или [`Virt`] соответственно.
    ///
    /// Возвращает ошибку [`Error::InvalidAlignment`] если `addr` не выровнен на [`Frage::SIZE`].
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn new(addr: Addr<T>) -> Result<Self> {
        Self::validate_size();

        if addr.into_usize().is_multiple_of(Self::SIZE) {
            Ok(Self(addr))
        } else {
            Err(InvalidAlignment)
        }
    }

    /// Возвращает нулевой [`Frage`] --- [`Frame`] или [`Page`].
    /// В отличие от [`Frage::default()`] доступна в константном контексте.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub const fn zero() -> Self {
        Self::validate_size();

        Self(Addr::zero())
    }

    /// Возвращает начальный адрес --- [`Phys`] или [`Virt`] ---
    /// для [`Frame`] или [`Page`] соответственно.
    pub fn address(&self) -> Addr<T> {
        self.0
    }

    /// Возвращает адрес [`Frage`] по его индексу `index`.
    ///
    /// Возвращает ошибку [`Error::Overflow`], если `index` превышает максимально допустимый.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn address_by_index(index: usize) -> Result<Addr<T>> {
        Self::validate_size();

        let address = index.checked_mul(Self::SIZE).ok_or(Overflow)?;
        Addr::new(address)
    }

    /// Создаёт [`Frage`] --- [`Frame`] или [`Page`] ---
    /// по находящемуся внутри адресу `addr` --- [`Phys`] или [`Virt`] соответственно.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn containing(addr: Addr<T>) -> Self {
        Self::validate_size();

        Self(Addr::new(addr.into_usize() & !(Self::SIZE - 1)).unwrap())
    }

    /// Создаёт [`Frage`] --- [`Frame`] или [`Page`] ---
    /// по его номеру `index`.
    ///
    /// Возвращает ошибку [`Error::Overflow`], если `index` превышает максимально допустимый.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn from_index(index: usize) -> Result<Self> {
        Self::validate_size();

        Ok(Self(Self::address_by_index(index)?))
    }

    /// Создаёт [`Frage`] --- [`Frame`] или [`Page`] ---
    /// по его номеру `index`.
    ///
    /// # Safety
    ///
    /// Номер `index` должен задавать допустимый [`Frage`].
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub unsafe fn from_index_unchecked(index: usize) -> Self {
        Self::validate_size();

        Self(unsafe { Addr::new_unchecked(index * Self::SIZE) })
    }

    /// Возвращает номер физического фрейма или виртуальной страницы.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn index(&self) -> usize {
        Self::validate_size();

        Self::index_by_address(self.address())
    }

    /// Возвращает номер физического фрейма или виртуальной страницы
    /// по адресу --- [`Phys`] или [`Virt`] соответственно.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn index_by_address(address: Addr<T>) -> usize {
        Self::validate_size();

        address.into_usize() / Self::SIZE
    }

    /// Возвращает требования к размещению в памяти блока
    /// физических фреймов или виртуальных страниц,
    /// достаточного для хранения объекта размером `size` **байт**.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn layout(size: usize) -> Result<Layout> {
        Self::validate_size();

        Ok(Layout::from_size_align(size, Self::SIZE)?)
    }

    /// Возвращает требования к размещению в памяти блока из `count`
    /// физических фреймов или виртуальных страниц.
    ///
    /// # Panic
    ///
    /// Паникует, если:
    /// - Полный размер --- `count * Self::SIZE` --- превышает `isize::MAX`.
    ///   См. требования метода [`Layout::from_size_align()`].
    /// - Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub const fn layout_array(count: usize) -> Layout {
        Self::validate_size();

        assert!(count <= (isize::MAX as usize) / Self::SIZE);
        unsafe { Layout::from_size_align_unchecked(count * Self::SIZE, Self::SIZE) }
    }

    /// Возвращает адрес в текущем фрейме или странице со смещением,
    /// равным остатку от деления заданного `address` на размер `Self::SIZE`.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    pub fn offset<Q: Tag>(
        &self,
        address: Addr<Q>,
    ) -> Addr<T> {
        Self::validate_size();

        (self.address() + address.into_usize() % Self::SIZE)
            .expect("offset inside a frame is always valid")
    }

    /// Возвращает размер физического фрейма или виртуальной страницы.
    pub const fn size(&self) -> usize {
        Self::SIZE
    }

    /// Проверяет, что размер текущего физического фрейма или виртуальной страницы поддерживается.
    ///
    /// # Panics
    ///
    /// Запрошенный размер физического фрейма или виртуальной страницы не поддерживается.
    const fn validate_size() {
        assert!(SIZE == L0_SIZE || SIZE == L1_SIZE || SIZE == L2_SIZE)
    }
}

impl<T: Tag, const SIZE: usize> Add<usize> for Frage<T, SIZE> {
    type Output = Result<Self>;

    fn add(
        self,
        rhs: usize,
    ) -> Self::Output {
        Self::new((self.0 + rhs * Self::SIZE)?)
    }
}

impl<T: Tag, const SIZE: usize> Sub<usize> for Frage<T, SIZE> {
    type Output = Result<Self>;

    fn sub(
        self,
        rhs: usize,
    ) -> Self::Output {
        Self::new((self.0 - rhs * Self::SIZE)?)
    }
}

impl<T: Tag, const SIZE: usize> Sub<Self> for Frage<T, SIZE> {
    type Output = Result<usize>;

    fn sub(
        self,
        rhs: Self,
    ) -> Self::Output {
        Ok((self.0 - rhs.0)? / Self::SIZE)
    }
}

impl<T: Tag, const SIZE: usize> fmt::Debug for Frage<T, SIZE> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}({}", T::FRAGE_NAME, self.index())?;

        if SIZE != L0_SIZE {
            write!(formatter, "<{}>", Size::bytes(SIZE))?;
        }

        write!(formatter, " @ {})", self.address())
    }
}

impl<T: Tag, const SIZE: usize> fmt::Display for Frage<T, SIZE> {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(formatter, "{}", self.index())?;

        if SIZE != L0_SIZE {
            write!(formatter, "<{}>", Size::bytes(SIZE))?;
        }

        write!(formatter, " @ {}", self.address())
    }
}

impl<T: Tag, const SIZE: usize> SizeOf for Frage<T, SIZE> {
    const SIZE_OF: usize = Self::SIZE;
}

/// (Физический) фрейм памяти.
pub type ElasticFrame<const SIZE: usize> = Frage<PhysTag, SIZE>;

/// (Виртуальная) страница памяти.
pub type ElasticPage<const SIZE: usize> = Frage<VirtTag, SIZE>;

impl<const SIZE: usize> ElasticPage<SIZE> {
    /// Возвращает индекс виртуальной страницы,
    /// которая следует через `page_count` страниц после текущей.
    /// Если такой нет, то возвращает индекс на единицу больше
    /// чем у последней виртуальной страницы --- [`Page::higher_half_end_index()`].
    pub fn advance_index(
        &self,
        page_count: usize,
    ) -> usize {
        Self::forward_checked(*self, page_count)
            .map(|page| page.index())
            .unwrap_or_else(Self::higher_half_end_index)
    }

    /// Возвращает `true` если виртуальная страница относится к
    /// [верхней половине](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details).
    pub fn is_higher_half(&self) -> bool {
        Self::higher_half_start_index() <= self.index()
    }

    /// Возвращает `true` если виртуальная страница относится к
    /// [нижней половине](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details).
    pub fn is_lower_half(&self) -> bool {
        self.index() < Self::lower_half_end_index()
    }

    /// Возвращает первую виртуальную страницу
    /// [верхней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn higher_half() -> Self {
        Self::containing(Virt::higher_half())
    }

    /// Возвращает первую виртуальную страницу
    /// [нижней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn lower_half() -> Self {
        Self::containing(Virt::lower_half())
    }

    /// Возвращает индекс первой виртуальной страницы
    /// [верхней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn higher_half_start_index() -> usize {
        Self::higher_half().index()
    }

    /// Возвращает индекс на единицу больше чем у последней виртуальной страницы
    /// [верхней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn higher_half_end_index() -> usize {
        (usize::MAX / Self::SIZE) + 1
    }

    /// Возвращает индекс первой виртуальной страницы
    /// [нижней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn lower_half_start_index() -> usize {
        Self::lower_half().index()
    }

    /// Возвращает индекс на единицу больше чем у последней виртуальной страницы
    /// [нижней половины](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    pub fn lower_half_end_index() -> usize {
        Self::lower_half_start_index() + Virt::half_size() / Self::SIZE
    }

    /// Возвращает запрещённый диапазон для номеров виртуальных страниц, находящийся между
    /// [двумя половинами](https://en.wikipedia.org/wiki/X86-64#Virtual_address_space_details)
    /// адресного пространства.
    fn non_canonical_range() -> Range<usize> {
        Self::lower_half_end_index() .. Self::higher_half_start_index()
    }
}

impl<const SIZE: usize> Step for ElasticPage<SIZE> {
    fn steps_between(
        start: &Self,
        end: &Self,
    ) -> (usize, Option<usize>) {
        if start <= end {
            let steps = if Self::is_lower_half(start) == Self::is_lower_half(end) {
                end.index() - start.index()
            } else {
                end.index() - start.index() - Self::non_canonical_range().count()
            };

            (steps, Some(steps))
        } else {
            (0, None)
        }
    }

    fn forward_checked(
        start: Self,
        count: usize,
    ) -> Option<Self> {
        let mut advanced = start.index().checked_add(count)?;

        if start.index() < Self::lower_half_end_index() && Self::lower_half_end_index() <= advanced
        {
            advanced = advanced.checked_add(Self::non_canonical_range().count())?;
        }

        Self::from_index(advanced).ok()
    }

    fn backward_checked(
        start: Self,
        count: usize,
    ) -> Option<Self> {
        let mut advanced = start.index().checked_sub(count)?;

        if advanced < Self::higher_half_start_index() &&
            Self::higher_half_start_index() <= start.index()
        {
            advanced = advanced.checked_sub(Self::non_canonical_range().count())?;
        }

        Some(Self::from_index(advanced).expect("wrong advancement formula"))
    }
}

/// (Физический) фрейм памяти с точки зрения MMU стандартного размера.
pub type L0Frame = ElasticFrame<L0_SIZE>;

/// (Виртуальная) страница памяти с точки зрения MMU стандартного размера.
pub type L0Page = ElasticPage<L0_SIZE>;

/// (Физический) фрейм памяти с точки зрения MMU большого размера.
///
/// А именно, соответствующий записи [`PageTableEntry`] первого уровня ---
/// [`PAGE_TABLE_LEAF_LEVEL + 1`] --- с установленным флагом [`PageTableFlags::HUGE`].
pub type L1Frame = ElasticFrame<L1_SIZE>;

/// (Виртуальная) страница памяти с точки зрения MMU большого размера.
///
/// А именно, соответствующий записи [`PageTableEntry`] первого уровня ---
/// [`PAGE_TABLE_LEAF_LEVEL + 1`] --- с установленным флагом [`PageTableFlags::HUGE`].
pub type L1Page = ElasticPage<L1_SIZE>;

/// (Физический) фрейм памяти с точки зрения MMU большого размера.
///
/// А именно, соответствующий записи [`PageTableEntry`] первого уровня ---
/// [`PAGE_TABLE_LEAF_LEVEL + 2`] --- с установленным флагом [`PageTableFlags::HUGE`].
pub type L2Frame = ElasticFrame<L2_SIZE>;

/// (Виртуальная) страница памяти с точки зрения MMU большого размера.
///
/// А именно, соответствующий записи [`PageTableEntry`] первого уровня ---
/// [`PAGE_TABLE_LEAF_LEVEL + 2`] --- с установленным флагом [`PageTableFlags::HUGE`].
pub type L2Page = ElasticPage<L2_SIZE>;

/// (Физический) фрейм памяти.
pub type Frame = L0Frame;

/// (Виртуальная) страница памяти.
pub type Page = L0Page;

/// Стандартный размер (виртуальных) страниц и (физических) фреймов памяти с точки зрения MMU.
/// Соответствует записи [`PageTableEntry`] нулевого уровня --- [`PAGE_TABLE_LEAF_LEVEL`].
pub const L0_SIZE: usize = 1 << PAGE_OFFSET_BITS;

/// Размер (виртуальных) страниц и (физических) фреймов,
/// соответствующий записи [`PageTableEntry`] первого уровня ---
/// [`PAGE_TABLE_LEAF_LEVEL + 1`] --- с установленным флагом [`PageTableFlags::HUGE`].
pub const L1_SIZE: usize = 1 << (PAGE_TABLE_INDEX_BITS + PAGE_OFFSET_BITS);

/// Размер (виртуальных) страниц и (физических) фреймов,
/// соответствующий записи [`PageTableEntry`] второго уровня ---
/// [`PAGE_TABLE_LEAF_LEVEL + 2`] --- с установленным флагом [`PageTableFlags::HUGE`].
pub const L2_SIZE: usize = 1 << (2 * PAGE_TABLE_INDEX_BITS + PAGE_OFFSET_BITS);

#[cfg(test)]
mod test {
    use static_assertions::const_assert_eq;

    use super::{
        super::{
            addr::{
                Phys,
                PhysTag,
                Tag,
                VirtTag,
            },
            mmu::{
                PAGE_OFFSET_BITS,
                PAGE_TABLE_INDEX_BITS,
            },
        },
        Addr,
        Frage,
        L0_SIZE,
        L0Page,
        L1_SIZE,
    };

    const_assert_eq!(L0Page::SIZE, 1 << PAGE_OFFSET_BITS);

    fn frage_alignment_check<T: Tag, const SIZE: usize>() {
        for offset in 1 .. Frage::<T, SIZE>::SIZE {
            assert!(Frage::<T, SIZE>::new(Addr::<T>::new(offset).unwrap()).is_err());
        }
    }

    #[test]
    fn alignment_check() {
        frage_alignment_check::<PhysTag, L0_SIZE>();
        frage_alignment_check::<VirtTag, L0_SIZE>();
        frage_alignment_check::<PhysTag, L1_SIZE>();
        frage_alignment_check::<VirtTag, L1_SIZE>();
    }

    fn frage_address_and_index<T: Tag, const SIZE: usize>(end_index: usize) {
        for index in (0 .. 10).chain(end_index - 10 .. end_index) {
            let frage = Frage::<T, SIZE>::from_index(index).unwrap();
            assert_eq!(frage.index(), index);
            assert_eq!(Frage::<T, SIZE>::index_by_address(frage.address()), index);
            assert_eq!(Frage::<T, SIZE>::new(frage.address()).unwrap(), frage);
        }

        let mut index = end_index;
        while index != 0 {
            assert!(Frage::<T, SIZE>::from_index(index).is_err());
            index <<= 1;
        }
    }

    #[test]
    fn address_and_index() {
        const ADDRESS_SPACE_BITS: u32 = usize::BITS;

        frage_address_and_index::<PhysTag, L0_SIZE>(1 << (Phys::BITS - PAGE_OFFSET_BITS));
        frage_address_and_index::<VirtTag, L0_SIZE>(1 << (ADDRESS_SPACE_BITS - PAGE_OFFSET_BITS));
        frage_address_and_index::<PhysTag, L1_SIZE>(
            1 << (Phys::BITS - PAGE_OFFSET_BITS - PAGE_TABLE_INDEX_BITS),
        );
        frage_address_and_index::<VirtTag, L1_SIZE>(
            1 << (ADDRESS_SPACE_BITS - PAGE_OFFSET_BITS - PAGE_TABLE_INDEX_BITS),
        );
    }
}
