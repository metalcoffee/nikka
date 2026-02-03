use core::{
    cell::Cell,
    fmt,
    fmt::Write,
    result,
};

use heapless::String;
use postcard::{
    Error::SerializeBufferFull,
    ser_flavors::Flavor,
};
use scopeguard::defer;
use serde::{
    Deserialize,
    Serialize,
    Serializer,
    ser::SerializeTuple,
};
use tracing::{
    Collect,
    Event,
    Id,
    Metadata,
    field::{
        Field,
        Visit,
    },
    span::{
        Attributes,
        Record,
    },
};
use tracing_core::span::Current;

use super::{
    RingBufferWriteTx,
    error::{
        Error,
        Error::InvalidArgument,
        Result,
    },
    pipe,
    time::{
        Tsc,
        datetime,
    },
};

pub use tracing::{
    Level,
    debug,
    error,
    event,
    info,
    trace,
    warn,
};

// Used in docs.
#[allow(unused)]
use crate as ku;

/// Структура для сериализации метаданных сообщения журнала, соответствует [`tracing::Metadata`].
///
/// Требуется, так как [`tracing::Metadata`] не реализует
/// типажи [`serde::Serialize`] и [`serde::Deserialize`].
/// Дополнительно снабжает метаданные сообщения отметкой времени.
#[derive(Debug, Deserialize, Serialize)]
pub struct LogMetadata<'a> {
    /// Исходный файл, где содержится вызов макроса, записавшего сообщение.
    file: Option<&'a str>,

    /// Строка исходного файла, где содержится вызов макроса, записавшего сообщение.
    line: Option<u32>,

    /// Уровень журналирования сообщения.
    level: char,

    /// Строка, описывающая часть программы, из которой сообщение было записано в журнал.
    /// По умолчанию --- имя модуля.
    /// Соответствует [`tracing::Metadata::target()`].
    target: &'a str,

    /// Отметка времени сообщения.
    timestamp: Tsc,
}

impl<'a> LogMetadata<'a> {
    /// Переводит метаданные `metadata` библиотеки [`tracing`] и отметку времени `timestamp`
    /// в сериализуемые метаданные [`LogMetadata`].
    pub fn new(
        metadata: &Metadata<'a>,
        timestamp: Tsc,
    ) -> Self {
        Self {
            file: metadata.file(),
            line: metadata.line(),
            level: level_into_symbol(metadata.level()),
            target: metadata.target(),
            timestamp,
        }
    }

    /// Исходный файл, где содержится вызов макроса, записавшего сообщение.
    pub fn file(&self) -> Option<&'a str> {
        self.file
    }

    /// Строка исходного файла, где содержится вызов макроса, записавшего сообщение.
    pub fn line(&self) -> Option<u32> {
        self.line
    }

    /// Уровень журналирования сообщения.
    pub fn level(&self) -> Result<Level> {
        level_try_from_symbol(self.level)
    }

    /// Строка, описывающая часть программы, из которой сообщение было записано в журнал.
    /// По умолчанию --- имя модуля.
    /// Соответствует [`tracing::Metadata::target()`].
    pub fn target(&self) -> &'a str {
        self.target
    }

    /// Отметка времени сообщения.
    pub fn timestamp(&self) -> Tsc {
        self.timestamp
    }
}

/// Поле сообщения с именем и значением, соответствует [`tracing::field::Field`].
/// Требуется, так как [`tracing::field::Field`] не реализует
/// типажи [`serde::Serialize`] и [`serde::Deserialize`].
#[derive(Debug, Deserialize)]
pub struct LogField<'a>(#[serde(borrow)] &'a str, #[serde(borrow)] LogFieldValue<'a>);

impl LogField<'_> {
    /// Имя поля сообщения.
    pub fn name(&self) -> &str {
        self.0
    }

    /// Значение поля сообщения.
    pub fn value(&self) -> &LogFieldValue<'_> {
        &self.1
    }
}

/// Значение поля сообщения.
#[derive(Debug, Deserialize, Serialize)]
pub enum LogFieldValue<'a> {
    /// Булево значение.
    Bool(bool),

    /// Знаковое целочисленное значение.
    I64(i64),

    /// Строковое значение.
    Str(&'a str),

    /// Беззнаковое целочисленное значение.
    U64(u64),

    /// Для значений остальных типов --- список строковых фрагментов, которые выдала
    /// соответствующая реализация [`core::fmt::Debug::fmt()`] при форматировании этого значения.
    VecStr,
}

/// Процесс сериализации одного сообщения журнала.
struct LogEvent<'a> {
    /// Сериализатор.
    serializer: postcard::Serializer<LogBuffer<'a>>,

    /// Результат сериализации.
    result: Result<()>,
}

impl LogEvent<'_> {
    /// Записывает сообщение `event` с отметкой времени `timestamp`
    /// в [`ku::info::ProcessInfo::log()`].
    fn record_event(
        event: &Event<'_>,
        timestamp: Tsc,
    ) -> Result<()> {
        if let Some(output) = LogBuffer::new() {
            let mut log_event = Self {
                serializer: postcard::Serializer { output },
                result: Ok(()),
            };

            log_event.result = log_event.record_header(event, timestamp);
            log_event.is_ok_so_far()?;

            event.record(&mut log_event);
            log_event.is_ok_so_far()?;

            log_event.serializer.output.buffer.commit();
        }

        Ok(())
    }

    /// Сериализует заголовок сообщения `event` с отметкой временем `timestamp`.
    fn record_header(
        &mut self,
        event: &Event<'_>,
        timestamp: Tsc,
    ) -> Result<()> {
        let metadata = LogMetadata::new(event.metadata(), timestamp);
        metadata.serialize(&mut self.serializer)?;

        let field_count = u8::try_from(event.fields().count())?;
        field_count.serialize(&mut self.serializer)?;

        Ok(())
    }

    /// Сериализует поле сообщения `field` со значением `value`.
    fn record_field(
        &mut self,
        field: &Field,
        value: &LogFieldValue<'_>,
    ) -> Result<()> {
        self.is_ok_so_far()?;

        let mut s = self.serializer.serialize_tuple(2)?;
        s.serialize_element(field.name())?;
        s.serialize_element(&value)?;
        s.end()?;

        Ok(())
    }

    /// Сериализует значение `value` поля сообщения через [`core::fmt::Debug::fmt()`].
    fn record_vec_str(
        &mut self,
        value: &dyn fmt::Debug,
    ) -> Result<()> {
        self.is_ok_so_far()?;

        self.write_fmt(format_args!("{value:?}"))?;

        Option::<&str>::serialize(&None, &mut self.serializer)?;

        Ok(())
    }

    /// Записывает один фрагмент `text` поля сообщения
    /// при сериализации через [`core::fmt::Debug::fmt()`].
    fn record_vec_element(
        &mut self,
        text: &str,
    ) -> Result<()> {
        self.is_ok_so_far()?;

        Some(text).serialize(&mut self.serializer)?;

        Ok(())
    }

    /// Запоминает первую ошибку, возникшую в процессе сериализации.
    /// Если `result` не содержит ошибку, ничего не делает.
    fn set_result(
        &mut self,
        result: Result<()>,
    ) {
        if self.result.is_ok() && result.is_err() {
            self.result = result;
        }
    }

    /// Возвращает [`Ok`] или возникшую в процессе сериализации ошибку.
    fn is_ok_so_far(&self) -> Result<()> {
        if self.serializer.output.result.is_err() {
            self.serializer.output.result?;
        }

        if self.result.is_err() {
            self.result.clone()?;
        }

        Ok(())
    }
}

impl Visit for LogEvent<'_> {
    fn record_debug(
        &mut self,
        field: &Field,
        value: &dyn fmt::Debug,
    ) {
        let result = self.record_field(field, &LogFieldValue::VecStr);
        self.set_result(result);

        let result = self.record_vec_str(value);
        self.set_result(result);
    }

    fn record_bool(
        &mut self,
        field: &Field,
        value: bool,
    ) {
        let result = self.record_field(field, &LogFieldValue::Bool(value));
        self.set_result(result);
    }

    fn record_i64(
        &mut self,
        field: &Field,
        value: i64,
    ) {
        let result = self.record_field(field, &LogFieldValue::I64(value));
        self.set_result(result);
    }

    fn record_str(
        &mut self,
        field: &Field,
        value: &str,
    ) {
        let result = self.record_field(field, &LogFieldValue::Str(value));
        self.set_result(result);
    }

    fn record_u64(
        &mut self,
        field: &Field,
        value: u64,
    ) {
        let result = self.record_field(field, &LogFieldValue::U64(value));
        self.set_result(result);
    }
}

impl Write for LogEvent<'_> {
    fn write_str(
        &mut self,
        text: &str,
    ) -> fmt::Result {
        let result = self.record_vec_element(text);
        if result.is_ok() {
            Ok(())
        } else {
            self.set_result(result);
            Err(fmt::Error)
        }
    }
}

/// Структура для записи префикса сообщения, которое не влезает в буфер целиком.
struct PlanB<const N: usize> {
    /// Буфер под строковое представление префикса сообщения.
    log_message: String<N>,

    /// Признак того, что буфер [`PlanB::log_message`] уже переполнен.
    overflow: bool,

    /// Признак того, что нужно записать разделитель полей после ранее записанного поля.
    separator: bool,
}

impl<const N: usize> PlanB<N> {
    /// Записывает префикс сообщения `event` в виде строки ограниченной длины.
    fn record_event(event: &Event<'_>) -> String<N> {
        let mut plan_b = Self {
            log_message: String::new(),
            overflow: false,
            separator: false,
        };

        event.record(&mut plan_b);

        plan_b.log_message
    }

    /// Сериализует поле сообщения `field` со значением `value` через [`core::fmt::Debug::fmt()`].
    fn record_field(
        &mut self,
        field: &Field,
        value: &dyn fmt::Debug,
    ) -> result::Result<(), ()> {
        if self.separator {
            self.log_message.push_str("; ")?;
        }
        self.separator = true;

        if field.name() != "message" {
            self.log_message.push_str(field.name())?;
            self.log_message.push_str(" = ")?;
        }

        self.write_fmt(format_args!("{value:?}")).map_err(|_| ())
    }
}

impl<const N: usize> Visit for PlanB<N> {
    fn record_debug(
        &mut self,
        field: &Field,
        value: &dyn fmt::Debug,
    ) {
        if !self.overflow {
            self.overflow = self.record_field(field, value).is_err();
        }
    }
}

impl<const N: usize> Write for PlanB<N> {
    fn write_str(
        &mut self,
        text: &str,
    ) -> fmt::Result {
        for ch in text.chars() {
            self.log_message.push(ch).map_err(|_| fmt::Error)?;
        }
        Ok(())
    }
}

/// Сборщик сообщений журнала.
pub struct LogCollector {
    /// Функция сброса буфера накопленных сообщений.
    /// Обычно это `syscall::sched_yield()`, так как за сброс буфера отвечает ядро.
    flush: Cell<Option<fn()>>,

    /// Текущий уровень журналирования.
    level: Level,

    /// Количество потерянных сообщений с момента предыдущего служебного сообщения о таких потерях.
    lost_recently: Cell<usize>,

    /// Количество потерянных сообщений за всё время.
    lost_totally: Cell<usize>,

    /// Количество потерянных сообщений, для которых к тому же провалился
    /// первый запасной вариант --- запись в журнал префикса сообщения фиксированной длины.
    plan_b_failures: Cell<usize>,

    /// Количество потерянных сообщений, для которых к тому же провалились оба запасных варианта.
    /// Первый запасной вариант записывает в журнал префикс сообщения фиксированной длины,
    /// второй запасной вариант --- только метаданные потерянного сообщения.
    plan_c_failures: Cell<usize>,

    /// Уровень вложенности текущей операции записи сообщения.
    /// В момент обработки записи сообщения, возможна попытка записать ещё одно сообщения.
    /// Поле [`LogCollector::recursion`] позволяет отсечь бесконечную рекурсию в этом случае.
    recursion: Cell<usize>,

    /// Количество потерянных сообщений в текущем наборе рекурсивных вызовов журналирования.
    recursive_failure: Cell<usize>,
}

impl LogCollector {
    /// Создаёт сборщик сообщений журнала, записывающий в [`ku::info::ProcessInfo::log()`]
    /// сообщения с уровнем журналирования `level` и выше.
    const fn new(level: Level) -> Self {
        LogCollector {
            flush: Cell::new(None),
            level,
            lost_recently: Cell::new(0),
            lost_totally: Cell::new(0),
            plan_b_failures: Cell::new(0),
            plan_c_failures: Cell::new(0),
            recursion: Cell::new(0),
            recursive_failure: Cell::new(0),
        }
    }

    /// Устанавливает функцию `flush` для сброса буфера накопленных сообщений.
    pub fn set_flush(
        &self,
        flush: fn(),
    ) {
        self.flush.set(Some(flush));
    }

    /// Возвращает `true` пока выполняется операция журналирования.
    ///
    /// Используется при обработки паник,
    /// чтобы понять что паника возможно возникла внутри подсистемы журналирования.
    /// А значит, журналировать саму панику стоит дополнительно запасным способом,
    /// минующим стандартную подсистему журналирования.
    pub fn is_buzy(&self) -> bool {
        self.recursion.get() != 0
    }

    /// Сбрасывает буфер накопленных сообщений.
    fn flush(&self) -> bool {
        if let Some(flush) = self.flush.get() {
            (flush)();
            true
        } else {
            false
        }
    }

    /// Записывает служебное сообщение с количеством потерянных сообщений.
    fn report_lost_messages_statistics(&self) {
        let recent = self.lost_recently.get();
        let recursive_failure = self.recursive_failure.get();

        if recent > 0 {
            let plan_b_failures = self.plan_b_failures.get();
            let plan_c_failures = self.plan_c_failures.get();

            self.lost_totally.update(|x| x + recent);
            let total = self.lost_totally.get();
            self.lost_recently.update(|x| x - recent);

            error!(
                recent,
                total, plan_b_failures, plan_c_failures, "lost some log messages",
            );

            if self.recursive_failure.get() > recursive_failure {
                // The error message about lost log messages seem to be lost also.
                // Rollback the statistics to trigger the same error message next time.
                self.lost_recently.update(|x| x + recent);
                self.lost_totally.update(|x| x - recent);
            }
        }
    }

    /// Записывает служебное сообщение с префиксом не поместившегося в буфер сообщения `event`,
    /// его метаданными, отметкой времени `timestamp` и
    /// возникшей при записи сообщения ошибкой `error`.
    /// Либо только с метаданными потерянного сообщения,
    /// отметкой времени `timestamp` и возникшей ошибкой `error`,
    /// если даже префикс исходного сообщения записать не удалось.
    fn report_lost_message(
        &self,
        event: &Event<'_>,
        timestamp: Tsc,
        error: &Error,
    ) {
        self.lost_recently.update(|x| x + 1);
        let recursive_failure = self.recursive_failure.get();

        let error_message = "failed to log an event";
        let message_prefix = PlanB::<PLAN_B_MAX_MESSAGE_SIZE>::record_event(event);
        error!(
            ?error,
            timestamp = %datetime(timestamp),
            metadata = ?event.metadata(),
            ?message_prefix,
            "{}",
            error_message,
        );

        if self.recursive_failure.get() > recursive_failure {
            self.plan_b_failures.update(|x| x + 1);
            error!(
                ?error,
                timestamp = %datetime(timestamp),
                metadata = ?event.metadata(),
                "{}",
                error_message,
            );
        }

        if self.recursive_failure.get() > recursive_failure + 1 {
            self.plan_c_failures.update(|x| x + 1);
        }
    }
}

// This is safe as long as user processes are single threaded.
unsafe impl Sync for LogCollector {
}

impl Collect for LogCollector {
    fn new_span(
        &self,
        _span: &Attributes<'_>,
    ) -> Id {
        Id::from_u64(0)
    }

    fn event(
        &self,
        event: &Event<'_>,
    ) {
        let timestamp = Tsc::now();

        self.recursion.update(|x| x + 1);
        let recursion = self.recursion.get();
        let is_recursive = recursion > 1;
        defer! {
            self.recursion.update(|x| x - 1);
        }

        /// Ограничение на рекурсивные вызовы журналирования.
        const RECURSION_LIMIT: usize = 3;

        if recursion > RECURSION_LIMIT {
            return;
        }

        /// Количество попыток записи сообщения в [`ku::info::ProcessInfo::log()`].
        /// Между которыми выполняется попытка сброса буфера.
        const TRY_COUNT: i32 = 2;

        for tries_left in (0 .. TRY_COUNT).rev() {
            if let Err(error) = LogEvent::record_event(event, timestamp) {
                if !self.flush() || tries_left == 0 {
                    if is_recursive {
                        self.recursive_failure.update(|x| x + 1);
                    } else {
                        self.report_lost_message(event, timestamp, &error);
                    }
                }
            } else {
                if !is_recursive {
                    self.recursive_failure.set(0);
                    self.report_lost_messages_statistics();
                }
                return;
            }
        }
    }

    fn record(
        &self,
        _span: &Id,
        _values: &Record<'_>,
    ) {
    }

    fn record_follows_from(
        &self,
        _span: &Id,
        _follows: &Id,
    ) {
    }

    fn enabled(
        &self,
        metadata: &Metadata<'_>,
    ) -> bool {
        metadata.level() <= &self.level
    }

    fn enter(
        &self,
        _span: &Id,
    ) {
    }

    fn exit(
        &self,
        _span: &Id,
    ) {
    }

    fn current_span(&self) -> Current {
        Current::unknown()
    }
}

/// Буфер для записи сообщения в [`ku::info::ProcessInfo::log()`].
struct LogBuffer<'a> {
    /// Буфер пишущей в [`ku::info::ProcessInfo::log()`] транзакции.
    buffer: RingBufferWriteTx<'a>,

    /// Текущий результат записи сообщения в буфер.
    result: pipe::Result<()>,
}

impl LogBuffer<'_> {
    /// Создаёт пишущую в [`ku::info::ProcessInfo::log()`] транзакцию и
    /// возвращает буфер для записи сообщения.
    fn new() -> Option<Self> {
        Some(Self {
            buffer: crate::process_info().log().write_tx()?,
            result: Ok(()),
        })
    }
}

impl Flavor for LogBuffer<'_> {
    type Output = ();

    fn try_extend(
        &mut self,
        data: &[u8],
    ) -> postcard::Result<()> {
        self.result = self.buffer.write(data);
        self.result.map_err(|_| SerializeBufferFull)
    }

    fn try_push(
        &mut self,
        data: u8,
    ) -> postcard::Result<()> {
        self.result = self.buffer.write(&[data; 1]);
        self.result.map_err(|_| SerializeBufferFull)
    }

    fn finalize(self) -> postcard::Result<Self::Output> {
        self.result.map_err(|_| SerializeBufferFull)
    }
}

/// Переводит уровень журналирования `level` в соответствующий символ.
/// Этот же символ используется при сериализации уровня журналирования.
pub const fn level_into_symbol(level: &Level) -> char {
    match *level {
        Level::ERROR => 'E',
        Level::WARN => 'W',
        Level::INFO => 'I',
        Level::DEBUG => 'D',
        Level::TRACE => 'T',
    }
}

/// Переводит символ уровня журналирования `level` в соответствующий уровень.
/// Возвращает ошибку [`Error::InvalidArgument`], если символу не соответствует никакой уровень.
pub const fn level_try_from_symbol(level: char) -> Result<Level> {
    match level {
        'E' => Ok(Level::ERROR),
        'W' => Ok(Level::WARN),
        'I' => Ok(Level::INFO),
        'D' => Ok(Level::DEBUG),
        'T' => Ok(Level::TRACE),
        _ => Err(InvalidArgument),
    }
}

/// Сборщик сообщений журнала.
pub static LOG_COLLECTOR: LogCollector = LogCollector::new(Level::DEBUG);

/// Размер префикса сообщения, который записывается в журнал в случае,
/// когда сообщение не влезает в буфер журнала целиком.
const PLAN_B_MAX_MESSAGE_SIZE: usize = 128;

/// Аналог [`std::dbg!()`](https://doc.rust-lang.org/std/macro.dbg.html).
#[macro_export]
macro_rules! dbg {
    () => {
        $crate::log::debug!("[{}:{}]", core::file!(), core::line!())
    };

    ($expression:expr $(,)?) => {
        match $expression {
            value => {
                $crate::log::debug!("[{}:{}] {} = {:#?}",
                    core::file!(), core::line!(), core::stringify!($expression), &value);
                value
            }
        }
    };

    ($($expression:expr),+ $(,)?) => {
        ($($crate::dbg!($expression)),+,)
    };
}
