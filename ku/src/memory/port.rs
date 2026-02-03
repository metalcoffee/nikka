use core::{
    marker::PhantomData,
    mem,
};

use x86::io;

use crate::error::{
    Error::{
        InvalidArgument,
        Overflow,
    },
    Result,
};

use super::{
    Addr,
    Tag,
    size,
};

// Used in docs.
#[allow(unused)]
use crate::error::Error;

/// [Порт ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O)
/// [архитектуры x86-64](https://wiki.osdev.org/X86-64).
pub type Port<T> = Addr<PortTag<T>>;

impl<T: PortData> Port<T> {
    /// Количество используемых битов в номерах
    /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    pub const BITS: u32 = 16;

    /// Количество неиспользуемых битов в номерах
    /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    const UNUSED_BITS: u32 = usize::BITS - Self::BITS;

    /// Создаёт
    /// [порт ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    /// по его битовому представлению `addr`.
    ///
    /// Возвращает ошибку [`Error::InvalidArgument`] если битовое представление `addr`
    /// не является корректным для номера порта ввода--вывода, работающего с данными типа `T`:
    ///   - Значение `addr` должно лежать от `0x0000` до `0xFFFF = (1 << Self::BITS) - 1`.
    ///   - Значение `addr` должно быть выровнено на размер типа `T`.
    fn new_impl(addr: usize) -> Result<Self> {
        let zeros = addr.leading_zeros();
        if zeros >= Self::UNUSED_BITS && addr.is_multiple_of(mem::size_of::<T>()) {
            Ok(unsafe { Self::new_unchecked_impl(addr) })
        } else {
            Err(InvalidArgument)
        }
    }

    /// Создаёт
    /// [порт ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    /// по его битовому представлению `addr`.
    ///
    /// # Safety
    ///
    /// Битовое представление `addr` должно являться корректным для номера порта ввода--вывода,
    /// работающего с данными типа `T`:
    ///   - Значение `addr` должно лежать от `0x0000` до `0xFFFF = (1 << Self::BITS) - 1`.
    ///   - Значение `addr` должно быть выровнено на размер типа `T`.
    unsafe fn new_unchecked_impl(addr: usize) -> Self {
        Self(addr, PhantomData)
    }

    /// Функция чтения из текущего
    /// [порта ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    ///
    /// # Safety
    ///
    /// Определяется спецификацией порта ввода--вывода.
    unsafe fn read(&self) -> T {
        unsafe { T::IN((*self).into()) }
    }

    /// Функция записи `value` в текущий
    /// [порт ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    ///
    /// # Safety
    ///
    /// Определяется спецификацией порта ввода--вывода.
    unsafe fn write(
        &self,
        value: T,
    ) {
        unsafe {
            T::OUT((*self).into(), value);
        }
    }
}

impl<T: PortData> From<Port<T>> for u16 {
    fn from(port: Port<T>) -> Self {
        u16::try_from(port.into_u64()).expect("port number should fit into u16")
    }
}

impl<T: PortData> TryFrom<u16> for Port<T> {
    type Error = Error;

    fn try_from(port: u16) -> Result<Self> {
        Port::new(size::from(port))
    }
}

/// Тип данных, которые можно записывать в
/// [порты ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
pub trait PortData: Clone + Copy + Default + Ord {
    /// Функция чтения из заданного
    /// [порта ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    ///
    /// # Safety
    ///
    /// Определяется спецификацией порта ввода--вывода.
    const IN: unsafe fn(u16) -> Self;

    /// Функция записи в заданный
    /// [порт ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    ///
    /// # Safety
    ///
    /// Определяется спецификацией порта ввода--вывода.
    const OUT: unsafe fn(u16, Self);
}

impl PortData for u8 {
    const IN: unsafe fn(u16) -> Self = io::inb;
    const OUT: unsafe fn(u16, Self) = io::outb;
}

impl PortData for u16 {
    const IN: unsafe fn(u16) -> Self = io::inw;
    const OUT: unsafe fn(u16, Self) = io::outw;
}

impl PortData for u32 {
    const IN: unsafe fn(u16) -> Self = io::inl;
    const OUT: unsafe fn(u16, Self) = io::outl;
}

/// Тег номеров
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct PortTag<T: PortData>(PhantomData<T>);

impl<T: PortData> Tag for PortTag<T> {
    const HEX_PREFIX: &'static str = "0x";
    const ADDR_NAME: &'static str = "Port";
    const BITS: u32 = Port::<T>::BITS;
    const FRAGE_NAME: &'static str = "Dock";

    fn new(addr: usize) -> Result<Addr<Self>> {
        Addr::<Self>::new_impl(addr)
    }

    unsafe fn new_unchecked(addr: usize) -> Addr<Self> {
        unsafe { Addr::<Self>::new_unchecked_impl(addr) }
    }

    fn is_same_half(
        _x: Addr<Self>,
        _y: Addr<Self>,
    ) -> bool {
        true
    }
}

/// Типаж пары индекс-данные
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
/// При обращении к регистру оборудования, его индекс записывается в индексный порт пары,
/// а данные записываются или читаются из порта данных пары.
///
/// Порты индекса и данных должны быть одинаковой ширины в байтах.
///
/// Типаж используется для того, чтобы в тестах можно было создать эмуляцию
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
/// И проверить корректность работы кода с портами.
/// В обычном же режиме используется реализация [`IndexDataPortPair`] этого типажа.
/// Которая работает с настоящими портами ввода--вывода.
pub trait IndexDataPair<T: PortData> {
    /// Читает данные из регистра оборудования `index`.
    ///
    /// # Safety
    ///
    /// Определяется спецификацией оборудования.
    unsafe fn read(
        &mut self,
        index: T,
    ) -> T;

    /// Записывает данные из регистра оборудования `index`.
    ///
    /// # Safety
    ///
    /// Определяется спецификацией оборудования.
    unsafe fn write(
        &mut self,
        index: T,
        data: T,
    );
}

/// Пара индекс-данные
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
/// При обращении к регистру оборудования, его индекс записывается в индексный порт пары,
/// а данные записываются или читаются из порта данных пары.
pub struct IndexDataPortPair<T: PortData> {
    /// Индексный порт ввода--вывода.
    index_port: Port<T>,

    /// Порт ввода--вывода для данных.
    data_port: Port<T>,
}

impl<T: PortData> IndexDataPortPair<T> {
    /// Возвращает пару индекс-данные
    /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    /// Индексный порт задаётся параметром `index_port`,
    /// порт данных --- параметром `data_port`.
    pub fn new(
        index_port: Port<T>,
        data_port: Port<T>,
    ) -> Self {
        Self {
            index_port,
            data_port,
        }
    }

    /// Возвращает пару индекс-данные смежных
    /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    /// Индексный порт задаётся параметром `index_port`,
    /// порт данных --- следующий порт за `index_port`, с учётом их ширины в байтах.
    pub fn from_index_port(index_port: u16) -> Result<Self> {
        Ok(Self::new(
            Port::try_from(index_port)?,
            Port::try_from(
                index_port
                    .checked_add(
                        u16::try_from(mem::size_of::<T>())
                            .expect("port data type should be u8, u16 or u32"),
                    )
                    .ok_or(Overflow)?,
            )?,
        ))
    }
}

impl<T: PortData> IndexDataPair<T> for IndexDataPortPair<T> {
    unsafe fn read(
        &mut self,
        index: T,
    ) -> T {
        unsafe {
            self.index_port.write(index);
            self.data_port.read()
        }
    }

    unsafe fn write(
        &mut self,
        index: T,
        data: T,
    ) {
        unsafe {
            self.index_port.write(index);
            self.data_port.write(data);
        }
    }
}

#[cfg(test)]
mod test {
    use super::Port;

    #[test]
    fn io_port_range() {
        assert!(Port::<u8>::new(0x0000).is_ok());
        assert!(Port::<u8>::new(0xFFFF).is_ok());
        assert!(Port::<u8>::new(0xFFFF + 1).is_err());
    }

    #[test]
    fn io_port_alignment() {
        assert!(Port::<u8>::new(0x0000).is_ok());
        assert!(Port::<u8>::new(0x0001).is_ok());
        assert!(Port::<u8>::new(0x0002).is_ok());
        assert!(Port::<u8>::new(0x0003).is_ok());
        assert!(Port::<u8>::new(0x0004).is_ok());

        assert!(Port::<u16>::new(0x0000).is_ok());
        assert!(Port::<u16>::new(0x0001).is_err());
        assert!(Port::<u16>::new(0x0002).is_ok());
        assert!(Port::<u16>::new(0x0003).is_err());
        assert!(Port::<u16>::new(0x0004).is_ok());

        assert!(Port::<u32>::new(0x0000).is_ok());
        assert!(Port::<u32>::new(0x0001).is_err());
        assert!(Port::<u32>::new(0x0002).is_err());
        assert!(Port::<u32>::new(0x0003).is_err());
        assert!(Port::<u32>::new(0x0000).is_ok());
    }
}
