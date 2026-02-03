#![allow(rustdoc::private_intra_doc_links)]

use core::{
    cmp,
    marker::PhantomData,
    mem,
    result,
    slice,
    sync::atomic::{
        AtomicU8,
        Ordering,
    },
};

use derive_getters::Getters;

use crate::{
    allocator::BigAllocator,
    error::{
        self,
        Error::InvalidAlignment,
    },
    log::{
        debug,
        error,
    },
    memory::{
        Block,
        Page,
        Virt,
        size,
    },
};

/// Создаёт однонаправленный канал для передачи последовательностей байт.
///
/// Отображает в память непрерывный циклический буфер [`RingBuffer`].
/// Использует для построения отображения `allocator`,
/// который должен реализовывать типаж постраничного аллокатора памяти
/// [`BigAllocator`].
///
/// Возвращает пару из [`ReadBuffer`] и [`WriteBuffer`] ---
/// интерфейса читателя и интерфейса писателя для этого буфера.
pub fn make<T: BigAllocator>(
    frame_count: usize,
    allocator: &mut T,
) -> error::Result<(ReadBuffer, WriteBuffer)> {
    // TODO: your code here.
    Ok((ReadBuffer::default(), WriteBuffer::default())) // TODO: remove before flight.
}

// ANCHOR: ring_buffer
/// [Непрерывный циклический буфер](https://fgiesen.wordpress.com/2012/07/21/the-magic-ring-buffer/).
#[derive(Debug, Default)]
pub struct RingBuffer<T: Tag> {
    /// Блок памяти с данными, которые хранятся в буфере.
    block: Block<Page>,

    /// Закрыт ли буфер.
    closed: bool,

    /// Размер заголовка одной записи [`RingBuffer`].
    header_size: usize,

    /// Количество байт, прочитанных из буфера за всё время.
    /// То есть, эта величина потенциально больше размера буфера.
    /// Вариант хранить в [`RingBuffer`] значения по модулю размера буфера, чреват ошибками.
    ///
    /// Аналогично, все методы работают с позициями в буфере,
    /// измеряя их от момента инициализации буфера.
    /// Единственное преобразование таких позиций в смещения в памяти, ---
    /// взятие по модулю `REAL_SIZE`, ---
    /// выполняется в реализации метода [`RingBuffer::get()`].
    head: usize,

    /// Количество байт, записанных в буфер за всё время.
    /// То есть, эта величина потенциально больше размера буфера.
    /// Вариант хранить в [`RingBuffer`] значения по модулю размера буфера, чреват ошибками.
    ///
    /// Аналогично, все методы работают с позициями в буфере,
    /// измеряя их от момента инициализации буфера.
    /// Единственное преобразование таких позиций в смещения в памяти, ---
    /// взятие по модулю `REAL_SIZE`, ---
    /// выполняется в реализации метода [`RingBuffer::get()`].
    tail: usize,

    /// Статистики чтения или записи в буфер.
    stats: RingBufferStats,

    /// Тег, отличающий писателя от читателя.
    _tag: PhantomData<T>,
}
// ANCHOR_END: ring_buffer

impl<T: Tag> RingBuffer<T> {
    /// Инициализирует буфер над блоком памяти `buf`.
    fn new(block: Block<Page>) -> Self {
        assert_ne!(block.start_address(), Virt::default());
        assert!(block.count().is_multiple_of(2));

        let mut header_size = mem::size_of::<AtomicU8>();
        let mut real_size = block.size() / 2;

        while real_size > 0 {
            header_size += mem::size_of::<u8>();
            real_size >>= u8::BITS;
        }

        Self {
            block,
            closed: false,
            header_size,
            head: 0,
            tail: 0,
            stats: RingBufferStats::default(),
            _tag: PhantomData,
        }
    }

    /// Возвращает блок виртуальной памяти, в которую отображён буфер.
    pub fn block(&self) -> Block<Page> {
        self.block
    }

    /// Закрывает [`RingBuffer`].
    /// После вызова этой функции любым из участников
    /// никакие транзакции больше создаваться не могут.
    /// А соответствующие методы [`ReadBuffer::read_tx()`] и [`WriteBuffer::write_tx()`]
    /// возвращают [`None`].
    ///
    /// Для упрощения, чтобы не реализовывать сложного протокола закрытия,
    /// этот метод работает асимметрично.
    /// Если буфер закрывает пишущая сторона, то читающая сначала прочитает все данные,
    /// которые были записаны в буфер до закрытия.
    /// Если же буфер закроет читающая сторона, то все данные, которые в него были записаны или
    /// писались в этот момент конкурентно, будут потеряны.
    pub fn close(&mut self) {
        // TODO: your code here.
        unimplemented!();
    }

    /// Максимальный размер полезной нагрузки в одной записи [`RingBuffer`].
    pub fn max_capacity(&self) -> usize {
        self.real_size() - self.header_size() - STATE_SIZE
    }

    /// Читает заголовок записи, находящейся на позиции `position`.
    /// В случае, если заголовок записи некорректен,
    /// считает что противоположная сторона закрыла буфер и возвращает
    /// [`Header::Closed`].
    fn read_header(
        &mut self,
        position: usize,
    ) -> Header {
        // TODO: your code here.
        unimplemented!();
    }

    /// Записывает заголовок записи, находящейся на позиции `position`.
    fn write_header(
        &mut self,
        position: usize,
        header: Header,
    ) {
        // TODO: your code here.
        unimplemented!();
    }

    /// Читает размер из заголовка записи, находящейся на позиции `position`.
    fn read_size(
        &mut self,
        position: usize,
    ) -> usize {
        let mut size = 0;
        for x in self.size(position) {
            size = (size << u8::BITS) | size::from(*x);
        }
        size
    }

    /// Записывает размер `size` в заголовок записи, находящейся на позиции `position`.
    fn write_size(
        &mut self,
        position: usize,
        size: usize,
    ) {
        let mut size = size;
        for x in self.size(position).iter_mut().rev() {
            *x = size as u8;
            size >>= u8::BITS;
        }
        assert_eq!(size, 0);
    }

    /// Возвращает ссылку на флаг состояния записи, находящейся на позиции `position`.
    ///
    /// Принимает `&mut self`, чтобы гарантировать отсутствие алиасинга.
    fn state(
        &mut self,
        position: usize,
    ) -> &AtomicU8 {
        AtomicU8::from_mut(self.get(position))
    }

    /// Возвращает ссылку на поле заголовка с размером записи,
    /// которая находится на позиции `position`.
    ///
    /// Принимает `&mut self`, чтобы гарантировать отсутствие алиасинга.
    fn size(
        &mut self,
        position: usize,
    ) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.get::<u8>(position + mem::size_of::<AtomicU8>()) as *mut u8,
                self.header_size() - mem::size_of::<AtomicU8>(),
            )
        }
    }

    /// Возвращает ссылку на буфер с данными, находящийся на позиции `position`.
    ///
    /// Принимает `&mut self`, чтобы гарантировать отсутствие алиасинга.
    fn buf(
        &mut self,
        position: usize,
    ) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.get::<u8>(position) as *mut u8, self.real_size()) }
    }

    /// Возвращает ссылку на тип `Q`, записанный на позиции `position`.
    /// Позиция `position` измеряется от момента инициализации буфера,
    /// то есть не учитывает переполнения размера буфера.
    /// Тип `Q` должен влезать в буфер и требовать тривиального выравнивания.
    ///
    /// # Panics
    ///
    /// Паникует, если:
    ///   - тип `Q` не влезает в буфер;
    ///   - тип `Q` требует нетривиального выравнивания.
    ///
    /// Принимает `&mut self`, чтобы гарантировать отсутствие алиасинга.
    fn get<Q>(
        &mut self,
        position: usize,
    ) -> &mut Q {
        assert!(mem::size_of::<Q>() <= self.real_size());

        let offset = position % self.real_size();
        let block = Block::<Virt>::from(self.block)
            .slice(offset .. offset + mem::size_of::<Q>())
            .expect("RingBuffer is not mapped properly");

        match unsafe { block.try_into_mut() } {
            Ok(value) => value,
            Err(InvalidAlignment) => panic!("type Q should be trivially aligned"),
            Err(error) => panic!("unexpected error {:?}", error),
        }
    }

    /// Позиция первого незафиксированного заголовка этой стороны буфера.
    fn header_position(&self) -> usize {
        if T::READ {
            self.head
        } else {
            self.tail
        }
    }

    // ANCHOR: header_size
    /// Размер заголовка одной записи [`RingBuffer`].
    fn header_size(&self) -> usize {
        self.header_size
    }
    // ANCHOR_END: header_size

    /// Размер отображённой виртуальной памяти.
    fn mapped_size(&self) -> usize {
        self.block.size()
    }

    /// Размер занимаемой физической памяти.
    fn real_size(&self) -> usize {
        self.mapped_size() / 2
    }
}

/// Читающая сторона [`RingBuffer`].
/// Позволяет создавать только читающие транзакции.
pub type ReadBuffer = RingBuffer<ReadTag>;

impl ReadBuffer {
    /// Создаёт читающую транзакцию.
    ///
    /// Возвращает [`None`], если [`RingBuffer`] был уже закрыт методом [`RingBuffer::close()`].
    pub fn read_tx(&mut self) -> Option<RingBufferReadTx<'_>> {
        if self.is_closed() {
            None
        } else {
            let head = self.head;
            let tail = self.tail;

            self.stats.txs += 1;

            Some(RingBufferTx {
                ring_buffer: self,
                head,
                tail,
                bytes: 0,
                _tag: PhantomData,
            })
        }
    }

    /// Возвращает статистики читающих транзакций.
    pub fn read_stats(&self) -> &RingBufferStats {
        &self.stats
    }

    /// Возвращает `true` если буфер закрыт.
    /// При этом обновляет информацию об этом от противоположного --- пишущего --- конца.
    fn is_closed(&mut self) -> bool {
        self.closed = self.closed || self.read_header(self.tail) == Header::Closed;
        self.closed
    }
}

/// Пишущая сторона [`RingBuffer`].
/// Позволяет создавать только пишущие транзакции.
pub type WriteBuffer = RingBuffer<WriteTag>;

impl WriteBuffer {
    /// Создаёт пишущую транзакцию.
    ///
    /// Возвращает [`None`], если [`RingBuffer`] был уже закрыт методом [`RingBuffer::close()`].
    pub fn write_tx(&mut self) -> Option<RingBufferWriteTx<'_>> {
        let header_size = self.header_size();
        let head = self.advance_head()?;
        let tail = self.tail + header_size;

        self.stats.txs += 1;

        Some(RingBufferTx {
            ring_buffer: self,
            head,
            tail,
            bytes: 0,
            _tag: PhantomData,
        })
    }

    /// Возвращает статистики пишущих транзакций.
    pub fn write_stats(&self) -> &RingBufferStats {
        &self.stats
    }

    // ANCHOR: advance_head
    /// Обновляет [`RingBuffer::head`] и возвращает его.
    ///
    /// Если буфер закрыт, возвращает [`None`].
    /// Если замечает нарушение инвариантов буфера,
    /// считает что противоположная сторона испортила буфер и расценивает его как закрытый.
    fn advance_head(&mut self) -> Option<usize> {
        // ANCHOR_END: advance_head
        // TODO: your code here.
        None // TODO: remove before flight.
    }
}

// ANCHOR: header
/// Заголовок одной записи [`RingBuffer`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum Header {
    /// Запись свободна, то есть в неё ещё ничего не записывалось.
    Clear = Self::CLEAR,

    /// Запись сообщает о закрытие буфера.
    Closed = Self::CLOSED,

    /// Запись зафиксирована писателем, но ещё не зафиксирована читателем.
    /// Так как она ещё нужна читателю, поверх неё пока нельзя писать новые данные,
    /// и она занимает место в буфере.
    Written {
        /// Размер полезных данных в записи.
        size: usize,
    } = Self::WRITTEN,

    /// Запись зафиксирована и писателем и читателем.
    /// Теперь поверх неё можно писать новые данные.
    Read {
        /// Размер полезных данных в записи.
        size: usize,
    } = Self::READ,
}
// ANCHOR_END: header

impl Header {
    /// Значение флага для свободной записи.
    /// См. [`Header::Clear`].
    const CLEAR: u8 = 0;

    /// Значение флага для записи, закрывающей [`RingBuffer`].
    /// См. [`Header::Closed`].
    const CLOSED: u8 = 3;

    /// Значение флага для зафиксированной писателем и не зафиксированной читателем записи.
    /// См. [`Header::Written`].
    const WRITTEN: u8 = 1;

    /// Значение флага для зафиксированной и писателем и читателем записи.
    /// См. [`Header::Written`].
    const READ: u8 = 2;
}

// ANCHOR: ring_buffer_tx
/// Читающая или пишущая транзакция.
#[derive(Debug)]
pub struct RingBufferTx<'a, T: Tag> {
    /// Ссылка на исходный [`RingBuffer`].
    ring_buffer: &'a mut RingBuffer<T>,

    /// Актуальное в рамках транзакции значение количества байт,
    /// прочитанных из буфера за всё время.
    /// В момент старта транзакции инициализируется из поля [`RingBuffer::head`].
    head: usize,

    /// Актуальное в рамках транзакции значение количества байт,
    /// записанных в буфер за всё время.
    /// В момент старта транзакции инициализируется из поля [`RingBuffer::tail`].
    tail: usize,

    /// Количество байт, прочитанных или записанных на текущий момент в данной транзакции.
    bytes: usize,

    /// Тег, отличающий пишущие транзакции от читающих.
    _tag: PhantomData<T>,
}
// ANCHOR_END: ring_buffer_tx

impl RingBufferTx<'_, ReadTag> {
    /// Возвращает в виде среза полезную нагрузку очередной записи из буфера.
    /// Или [`None`], если в этой читающей транзакции больше нет записей.
    /// Обновляет поля только самой транзакции [`RingBufferTx`].
    ///
    /// # Safety
    ///
    /// Так как возвращается срез, расположенный в разделяемой памяти,
    /// его содержимое может конкурентно меняться другой стороной.
    /// То есть, нельзя проверить его на какие-либо инварианты,
    /// и после этого использовать, опираясь на них.
    /// Так как в промежутке эти инварианты могут быть сломаны другой стороной.
    ///
    /// То есть, нужно:
    ///   - Либо гарантировать, что другая сторона не запускается конкурентно.
    ///   - Либо прежде всего скопировать срез, и дальше пользоваться только копией.
    pub unsafe fn read(&mut self) -> Option<&[u8]> {
        // TODO: your code here.
        unimplemented!();
    }

    /// Фиксирует читающую транзакцию, записывая обновлённое значение [`RingBuffer::head`] и
    /// статистику [`RingBuffer::stats`] в поля [`RingBuffer`].
    #[allow(unused_mut)] // TODO: remove before flight.
    pub fn commit(mut self) {
        // TODO: your code here.
        unimplemented!();
    }
}

impl RingBufferTx<'_, WriteTag> {
    /// Копирует в буфер байты среза `data`.
    /// Обновляет поля самой транзакции [`RingBufferTx`] и статистики [`RingBuffer::stats`],
    /// но не трогает поля [`RingBuffer::head`] и [`RingBuffer::tail`].
    /// Если в буфере не остаётся места под `data`, возвращает ошибку [`Error::Overflow`].
    pub fn write(
        &mut self,
        data: &[u8],
    ) -> Result<()> {
        // TODO: your code here.
        unimplemented!();
    }

    /// Фиксирует пишущую транзакцию, обновляя значение [`RingBuffer::tail`] и
    /// статистику [`RingBuffer::stats`] в полях [`RingBuffer`].
    #[allow(unused_mut)] // TODO: remove before flight.
    pub fn commit(mut self) {
        // TODO: your code here.
        unimplemented!();
    }

    /// Ёмкость, оставшаяся в буфере транзакции на текущий момент.
    pub fn capacity(&mut self) -> usize {
        self.try_advance_head();

        self.ring_buffer
            .real_size()
            .saturating_sub((self.tail - self.head) + STATE_SIZE)
    }

    /// Пытается продвинуть [`RingBufferWriteTx::head`],
    /// на случай что читающая сторона конкурентно зафиксировала новые [`RingBufferReadTx`].
    fn try_advance_head(&mut self) {
        if let Some(head) = self.ring_buffer.advance_head() {
            self.head = head;
        }
    }
}

impl<T: Tag> Drop for RingBufferTx<'_, T> {
    /// Обрывает читающую или пишущую транзакцию.
    /// Обновляет статистики [`RingBuffer::stats`],
    /// если в транзакции был прочитан или записан хотя бы один байт.
    fn drop(&mut self) {
        // TODO: your code here.
        unimplemented!();
    }
}

/// Тег, отличающий пишущие транзакции от читающих.
pub trait Tag {
    /// `true` если это писатель.
    const READ: bool;
}

/// Читающая транзакция.
pub type RingBufferReadTx<'a> = RingBufferTx<'a, ReadTag>;

/// Пишущая транзакция.
pub type RingBufferWriteTx<'a> = RingBufferTx<'a, WriteTag>;

/// Тег читающих транзакций.
#[derive(Debug, Default)]
pub struct ReadTag;

impl Tag for ReadTag {
    const READ: bool = true;
}

/// Тег пишущих транзакций.
#[derive(Debug, Default)]
pub struct WriteTag;

impl Tag for WriteTag {
    const READ: bool = false;
}

// ANCHOR: ring_buffer_stats
/// Статистики чтения или записи в буфер.
#[derive(Clone, Copy, Debug, Default, Eq, Getters, PartialEq)]
pub struct RingBufferStats {
    /// Количество байт, прочитанных или записанных в зафиксированных транзакциях.
    committed: usize,

    /// Количество зафиксированных транзакций соответствующего типа.
    commits: usize,

    /// Количество байт, прочитанных или записанных в оборванных транзакциях.
    dropped: usize,

    /// Количество отменённых транзакций соответствующего типа.
    drops: usize,

    /// Количество ошибок в транзакциях.
    errors: usize,

    /// Количество транзакций чтения либо записи соответственно.
    txs: usize,
}
// ANCHOR_END: ring_buffer_stats

// ANCHOR: error
/// Ошибки, которые могут возникать при работе с [`RingBuffer`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Буфер транзакции переполнен.
    Overflow {
        /// Место, остававшееся в буфере на момент старта транзакции.
        /// То есть, полная доступная для транзакции ёмкость.
        capacity: usize,

        /// Объём данных, уже записанных ранее в рамках транзакции.
        len: usize,

        /// Размер объекта, при записи которого буфер транзакции переполнился.
        /// Должно выполняться неравенство `exceeding_object_len > capacity - len`.
        exceeding_object_len: usize,
    },
}
// ANCHOR_END: error

/// Тип возвращаемого результата `T` или ошибки [`Error`] ---
/// мономорфизация [`result::Result`] по типу ошибки.
pub type Result<T> = result::Result<T, Error>;

/// Размер хвоста после конца записи в буфере --- флага состояния следующей записи.
/// Используется при записи в буфер, чтобы отметить следующую запись свободной.
/// То есть, чтобы читающая сторона не пыталась читать следующую запись до тех пор,
/// пока та не будет записана на самом деле.
const STATE_SIZE: usize = mem::size_of::<AtomicU8>();

#[doc(hidden)]
pub mod test_scaffolding {
    use crate::memory::{
        Block,
        Page,
    };

    use super::{
        Header,
        RingBuffer,
        RingBufferStats,
        RingBufferTx,
        Tag,
    };

    pub fn block<T: Tag>(buffer: &RingBuffer<T>) -> Block<Page> {
        buffer.block()
    }

    pub fn head_tail<T: Tag>(buffer: &RingBuffer<T>) -> (usize, usize) {
        (
            buffer.head % buffer.real_size(),
            buffer.tail % buffer.real_size(),
        )
    }

    pub fn header_size<T: Tag>(buffer: &RingBuffer<T>) -> usize {
        buffer.header_size()
    }

    pub fn stats<T: Tag>(buffer: &RingBuffer<T>) -> RingBufferStats {
        buffer.stats
    }

    pub fn tx_head_tail<'a, T: Tag>(tx: &'a RingBufferTx<'a, T>) -> (usize, usize) {
        (
            tx.head % tx.ring_buffer.real_size(),
            tx.tail % tx.ring_buffer.real_size(),
        )
    }

    pub const CLEAR: u8 = Header::CLEAR;
    pub const CLOSED: u8 = Header::CLOSED;
    pub const WRITTEN: u8 = Header::WRITTEN;
    pub const READ: u8 = Header::READ;
}
