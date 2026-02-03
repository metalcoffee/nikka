use core::fmt::{
    Debug,
    Write,
};

use serde::Deserialize;
use tracing::{
    Collect,
    Event,
    Id,
    Level,
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
use tracing_core::{
    dispatch,
    dispatch::Dispatch,
    span::Current,
};

use ku::{
    ReadBuffer,
    log::{
        LogField,
        LogFieldValue,
        LogMetadata,
        level_into_symbol,
    },
    sync::{
        PanicStrategy,
        Spinlock,
    },
    time::{
        Tsc,
        datetime_ms,
    },
};
use text::{
    Color,
    print,
    println,
};

use crate::{
    error::{
        Error::Unimplemented,
        Result,
    },
    process::Pid,
    smp::LocalApic,
};

pub use tracing::{
    debug,
    error,
    info,
    trace,
    warn,
};

/// Инициализация журналирования.
pub(super) fn init() {
    dispatch::set_global_default(Dispatch::from_static(&LOG_COLLECTOR)).unwrap();
}

/// Записывает в журнал все сообщения от пользовательского процесса `pid`,
/// сохранённые им в буфер `log`.
pub(super) fn user_events(
    pid: Pid,
    log: &mut ReadBuffer,
) {
    LOG_COLLECTOR.log.lock().user_events(pid, log);
}

/// Вспомогательная структура для печати сообщения.
struct LogEvent {
    /// Признак того, что нужно записать разделитель полей после ранее записанного поля.
    separator: bool,

    /// Текущий цвет при выводе части сообщения на экран.
    color: Color,
}

impl LogEvent {
    /// Цвет для вывода текста сообщения.
    const MESSAGE: Color = Color::WHITE;

    /// Цвет для вывода значений полей сообщения.
    const VALUE: Color = Color::LIGHT_CYAN;

    /// Создаёт вспомогательную структуру для печати сообщения.
    fn new() -> Self {
        Self {
            separator: false,
            color: Self::VALUE,
        }
    }

    /// Печатает заголовок поля `name`.
    /// Если `name == "message"`, то это поле --- текст сообщения.
    /// Для него имя поля опускается.
    fn field(
        &mut self,
        name: &str,
    ) {
        if self.separator {
            print!("; ");
        } else {
            self.separator = true
        }

        if name == "message" {
            self.color = Self::MESSAGE;
        } else {
            print!("{} = ", name);
            self.color = Self::VALUE;
        }
    }

    /// Печатает строковый фрагмент `value_part` значения поля сообщения.
    fn str_value_part(
        &mut self,
        value_part: &str,
    ) {
        print!(color(self.color), "{}", value_part);
    }

    /// Печатает поле сообщения `name` со значением `value` через [`core::fmt::Debug::fmt()`].
    fn debug(
        &mut self,
        name: &str,
        value: &dyn Debug,
    ) {
        self.field(name);
        print!(color(self.color), "{:?}", value);
    }
}

impl Visit for LogEvent {
    fn record_debug(
        &mut self,
        field: &Field,
        value: &dyn Debug,
    ) {
        self.debug(field.name(), value);
    }
}

/// Формат печати сообщений журнала.
#[allow(unused)]
#[derive(Eq, PartialEq)]
enum Format {
    /// Компактный формат:
    /// ```console
    /// <время в UTC> <CPU id> <level char> <message>; <key1> = <value1>; <key2> = <value2>; ...
    /// ```
    /// Пример:
    /// ```console
    /// 17:50:31.233 0 I Nikka booted; now = 2023-01-01 17:50:31 UTC; tsc = Tsc(2850348853)
    /// ```
    ///
    /// - Сокращает уровень журналирования до первой буквы.
    /// - Не печатает [`tracing::Metadata::target()`].
    /// - Не печатает файл [`tracing::Metadata::file()`] и строку [`tracing::Metadata::line()`]
    ///   исходного кода, где находится соответствующий вызов макроса журналирования.
    Compact,

    /// Полный формат:
    /// ```console
    /// <время в UTC> <CPU id> <level> <target> <file:line> <message>; <key1> = <value1>; <key2> = <value2>; ...
    /// ```
    /// Пример:
    /// ```console
    /// 17:51:20.477 0 INFO kernel kernel/src/lib.rs:118 Nikka booted; now = 2023-01-01 17:51:20 UTC; tsc = Tsc(2732927919)
    /// ```
    Full,

    /// Аналогичен [`Format::Compact`], но дополнительно не печатает время:
    /// ```console
    /// <CPU id> <level char> <message>; <key1> = <value1>; <key2> = <value2>; ...
    /// ```
    /// Это позволяет писать в журнал из функций, которые вычисляют текущее время, без зацикливания.
    /// Пример:
    /// ```console
    /// 0 I Nikka booted; now = 2023-01-01 17:52:07 UTC; tsc = Tsc(2787412607)
    /// ```
    ///
    /// - Не печатает текущее время.
    /// - Сокращает уровень журналирования до первой буквы.
    /// - Не печатает [`tracing::Metadata::target()`].
    /// - Не печатает файл [`tracing::Metadata::file()`] и строку [`tracing::Metadata::line()`]
    ///   исходного кода, где находится соответствующий вызов макроса журналирования.
    Timeless,
}

/// Сборщик записей журнала для печати сообщений в заданном формате.
struct Log {
    /// Формат печати сообщений журнала.
    format: Format,
}

impl Log {
    /// Цвет для вывода номера CPU.
    const CPU: Color = Color::DARK_GRAY;

    /// Цвет для вывода [`tracing::Metadata::target()`],
    /// файла [`tracing::Metadata::file()`] и строки [`tracing::Metadata::line()`].
    const LOCATION: Color = Color::DARK_GRAY;

    /// Создаёт сборщик записей журнала для печати сообщений в формате `format`.
    const fn new(format: Format) -> Self {
        Self { format }
    }

    /// Возвращает цвет, которым нужно печатать уровень журналирования `level`.
    const fn level_color(level: &Level) -> Color {
        match *level {
            Level::ERROR => Color::LIGHT_RED,
            Level::WARN => Color::LIGHT_YELLOW,
            Level::INFO => Color::WHITE,
            Level::DEBUG => Color::LIGHT_BLUE,
            Level::TRACE => Color::DARK_GRAY,
        }
    }

    /// Печатает сообщение `event` с отметкой времени `timestamp`.
    fn log_event(
        &self,
        event: &Event<'_>,
        timestamp: Tsc,
    ) {
        self.log_metadata(
            event.metadata().level(),
            LogMetadata::new(event.metadata(), timestamp),
        );
        event.record(&mut LogEvent::new());
        println!();
    }

    /// Печатает метаданные `metadata` сообщения, включая отметку времени,
    /// на уровне журналирования `level`.
    fn log_metadata(
        &self,
        level: &Level,
        metadata: LogMetadata,
    ) {
        if self.format != Format::Timeless {
            let timestamp = datetime_ms(metadata.timestamp());
            print!("{:?} ", timestamp.time());
        }

        print!(color(Self::CPU), "{} ", LocalApic::id());

        match self.format {
            Format::Compact | Format::Timeless => {
                print!(
                    color(Self::level_color(level)),
                    "{} ",
                    level_into_symbol(level),
                );
            },
            Format::Full => {
                print!(color(Self::level_color(level)), "{} ", level);
                print!(
                    color(Self::LOCATION),
                    "{} {}:{} ",
                    metadata.target(),
                    metadata.file().unwrap_or("?"),
                    metadata.line().unwrap_or(0),
                );
            },
        }
    }

    /// Печатает все сообщения от пользовательского процесса `pid`,
    /// сериализованные им в буфер `log`.
    fn user_events(
        &self,
        pid: Pid,
        log: &mut ReadBuffer,
    ) {
        if let Some(mut tx) = log.read_tx() {
            while let Some(event) = unsafe { tx.read() } {
                let mut deserializer = postcard::Deserializer::from_bytes(event);
                if self.user_event(pid, &mut deserializer).is_err() {
                    return;
                }
            }

            tx.commit();
        }
    }

    /// Печатает одно сообщение от пользовательского процесса `pid`,
    /// десериализуя его из `deserializer`.
    fn user_event<'a>(
        &self,
        pid: Pid,
        deserializer: &mut postcard::Deserializer<'a, postcard::de_flavors::Slice<'a>>,
    ) -> Result<()> {
        let metadata = LogMetadata::deserialize(&mut *deserializer)?;
        let level = metadata.level().map_err(|_| Unimplemented)?;
        self.log_metadata(&level, metadata);

        let count = u8::deserialize(&mut *deserializer)?;
        let mut event = LogEvent::new();
        for _ in 0 .. count {
            let field = LogField::deserialize(&mut *deserializer)?;
            match field.value() {
                LogFieldValue::VecStr => {
                    event.field(field.name());
                    while let Some(value) = Option::<&str>::deserialize(&mut *deserializer)? {
                        event.str_value_part(value);
                    }
                },
                LogFieldValue::I64(value) => event.debug(field.name(), &value as &dyn Debug),
                LogFieldValue::U64(value) => event.debug(field.name(), &value as &dyn Debug),
                LogFieldValue::Bool(value) => event.debug(field.name(), &value as &dyn Debug),
                LogFieldValue::Str(value) => event.debug(field.name(), &value as &dyn Debug),
            }
        }
        event.debug("pid", &pid as &dyn Debug);

        println!();

        Ok(())
    }
}

/// Сборщик сообщений журнала, печатающий сообщения на экран и в COM--порт.
struct LogCollector {
    /// Уровень журналирования.
    /// Печатаются только сообщения с уровнем журналирования, равным [`LogCollector::level`] и выше.
    level: Level,

    /// Сборщик записей журнала для печати сообщений в заданном формате.
    log: Spinlock<Log, { PanicStrategy::KnockDown }>,
}

impl LogCollector {
    /// Создаёт сборщик сообщений журнала, печатающий сообщения с уровнем журналирования `level` и выше
    /// на экран и в COM--порт в формате `format`.
    const fn new(
        format: Format,
        level: Level,
    ) -> Self {
        Self {
            level,
            log: Spinlock::new(Log::new(format)),
        }
    }
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
        let now = Tsc::now();
        self.log.lock().log_event(event, now);
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

/// Сборщик сообщений журнала, печатающий сообщения на экран и в COM--порт.
static LOG_COLLECTOR: LogCollector = LogCollector::new(Format::Compact, Level::DEBUG);
