#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

//! Библиотека, реализующая печать на экране в текстовом режиме графического контроллера
//! [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).

#![deny(warnings)]
#![no_std]
#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]

use core::fmt::{
    Result,
    Write,
};

use bitflags::bitflags;
use derive_more::{
    Deref,
    DerefMut,
};
use lazy_static::lazy_static;
use volatile::Volatile;

use serial::{
    Com,
    Serial,
};

use ku::{
    memory::IndexDataPortPair,
    sync::{
        IrqSpinlock,
        PanicStrategy,
    },
};

use cursor::VgaCursor;
use grid::{
    GlyphWrapper,
    Grid,
};

pub use cursor::Cursor;
pub use grid::Glyph;

/// Управление курсором в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
mod cursor;

/// Работа с содержимым экрана в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
mod grid;

/// Тесты.
#[cfg(test)]
mod test;

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    /// Цвета символов и фона в текстовом режиме графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
    /// [Таблица соответствия RGB](https://wiki.osdev.org/Printing_To_Screen#Color_Table).
    pub struct Color: u8 {
        /// Чёрный --- `#000000`.
        const BLACK = 0;

        /// Красный --- `#AA0000`.
        const RED = 1 << 2;

        /// Зелёный --- `#00AA00`.
        const GREEN = 1 << 1;

        /// Синий --- `#0000AA`.
        const BLUE = 1 << 0;

        /// Циановый (зелёный + синий) --- `#00AAAA`.
        const CYAN = Self::GREEN.bits() | Self::BLUE.bits();

        /// Маджента (красный + синий) --- `#AA00AA`.
        const MAGENTA = Self::RED.bits() | Self::BLUE.bits();

        /// Коричневый (красный + ослабленный зелёный) --- `#AA5500`.
        const BROWN = Self::RED.bits() | Self::GREEN.bits();

        /// Коричневый (красный + ослабленный зелёный) --- `#AA5500`.
        const YELLOW = Self::BROWN.bits();

        /// Серый (красный + зелёный + синий) --- `#AAAAAA`.
        const GRAY = Self::RED.bits() | Self::GREEN.bits() | Self::BLUE.bits();

        /// Флаг яркости цвета.
        const LIGHT = 1 << 3;

        /// Тёмно--серый (технически яркий + чёрный) --- `#555555`.
        const DARK_GRAY = Self::LIGHT.bits() | Self::BLACK.bits();

        /// Ярко--красный (яркий + красный) --- `#FF5555`.
        const LIGHT_RED = Self::LIGHT.bits() | Self::RED.bits();

        /// Ярко--зелёный (яркий + зелёный) --- `#55FF55`.
        const LIGHT_GREEN = Self::LIGHT.bits() | Self::GREEN.bits();

        /// Ярко--синий (яркий + синий) --- `#5555FF`.
        const LIGHT_BLUE = Self::LIGHT.bits() | Self::BLUE.bits();

        /// Ярко--циановый (яркий + зелёный + синий) --- `#55FFFF`.
        const LIGHT_CYAN = Self::LIGHT.bits() | Self::CYAN.bits();

        /// Яркая маджента (яркий + красный + синий) --- `#FF55FF`.
        const LIGHT_MAGENTA = Self::LIGHT.bits() | Self::MAGENTA.bits();

        /// Ярко--жёлтый (яркий + красный + зелёный) --- `#FFFF55`.
        const LIGHT_YELLOW = Self::LIGHT.bits() | Self::YELLOW.bits();

        /// Белый (технически яркий + серый = яркий + красный + зелёный + синий) --- `#FFFFFF`.
        const WHITE = Self::LIGHT.bits() | Self::GRAY.bits();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
/// Тип для атрибутов, с которыми отображаются символы.
pub struct Attribute(u8);

impl Attribute {
    /// Возвращает атрибуты для символа, имеющего цвет `foreground` на фоне цвета `background`.
    pub const fn new(
        foreground: Color,
        background: Color,
    ) -> Attribute {
        Attribute(background.bits() << Self::BACKGROUND_SHIFT | foreground.bits())
    }

    /// Возвращает цвет фона.
    pub const fn background(&self) -> Color {
        Color::from_bits(self.0 >> Self::BACKGROUND_SHIFT).expect("undefined color")
    }

    /// Битовый сдвиг для цвета фона в байте атрибутов символа.
    const BACKGROUND_SHIFT: u8 = 4;
}

/// Структура, позволяющая печатать на экран в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
/// И одновременно выводить печатаемые символы в
/// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
/// для отладочных целей.
#[derive(Deref, DerefMut)]
pub struct Text<'a, C: Cursor, S: Serial> {
    /// Управление курсором.
    cursor: C,

    /// Управление содержимым экрана.
    #[deref]
    #[deref_mut]
    grid: Grid<'a>,

    /// [Последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
    /// для отладочных целей.
    serial: S,
}

impl<'a, C: Cursor, S: Serial> Text<'a, C, S> {
    /// Возвращает структуру,
    /// позволяющую печатать на экран в текстовом режиме графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
    /// И одновременно выводить печатаемые символы в
    /// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
    /// для отладочных целей.
    ///
    /// - Аргумент `grid` задаёт структуру, которая описывает содержимое экрана, см. [`Grid`].
    /// - Аргумент `cursor` задаёт курсор, см. [`VgaCursor`].
    /// - Аргумент `serial` задаёт
    ///   [последовательный порт](https://en.wikipedia.org/wiki/Serial_port),
    ///   см. [`Serial`].
    fn new(
        grid: Grid<'a>,
        cursor: C,
        serial: S,
    ) -> Self {
        Self {
            grid,
            cursor,
            serial,
        }
    }

    /// Управление курсором.
    pub fn cursor_mut(&mut self) -> &mut C {
        &mut self.cursor
    }

    /// Устанавливает текущую позицию.
    /// То есть, положение в котором находится курсор и
    /// в котором будет напечатан следующий символ.
    pub fn set_position(
        &mut self,
        position: usize,
    ) {
        self.grid.set_position(position);
        self.cursor.set(position);
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Инициализирует структуру по текущему содержимому экрана.
    /// Само содержимое экрана копирует в
    /// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
    /// [`Text::serial`], игнорируя атрибуты [`Glyph::attribute`].
    ///
    /// Вычисляет текущую позицию на экране как максимальную из:
    /// - Текущей позиции курсора [`Text::cursor`].
    /// - И максимальной позицией, перед которой отображён печатный символов.
    pub fn init(&mut self) {
        let position = self.grid.init(self.cursor.get(), &mut self.serial);

        let is_screen_clear = position == 0;
        if !is_screen_clear {
            if !self.grid.is_newline() {
                writeln!(self).unwrap();
            }

            writeln!(self).unwrap();
        }
    }

    /// Очищает экран. Для этого заполняет его пробелами с текущими атрибутами.
    pub fn clear(&mut self) {
        self.grid.clear(0 .. self.grid.len());
        self.set_position(0);
    }
}

impl<'a, C: Cursor, S: Serial> Write for Text<'a, C, S> {
    fn write_str(
        &mut self,
        text: &str,
    ) -> Result {
        for ch in text.chars() {
            self.grid.print_character(ch)
        }
        for octet in text.as_bytes() {
            self.serial.print_octet(*octet);
        }

        self.cursor.set(self.grid.position());

        Ok(())
    }
}

/// Структура, позволяющую печатать на экран в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
/// И одновременно выводить печатаемые символы в
/// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
/// для отладочных целей.
type VgaText = Text<'static, VgaCursor<IndexDataPortPair<u8>>, Com>;

lazy_static! {
    /// Структура, позволяющую печатать на экран в текстовом режиме графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
    /// И одновременно выводить печатаемые символы в
    /// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
    /// для отладочных целей.
    pub static ref TEXT: IrqSpinlock<VgaText, { PanicStrategy::KnockDown }> = {
        const COLUMN_COUNT: usize = 80;
        const ROW_COUNT: usize = 25;
        let buffer = 0xB_8000 as *mut [Volatile<GlyphWrapper>; COLUMN_COUNT * ROW_COUNT];
        let tab_width = 8;
        let grid = Grid::new(unsafe { &mut *buffer }, COLUMN_COUNT, ROW_COUNT, tab_width);

        let mut cursor = cursor::create_vga_cursor();
        cursor.set_height(2);

        IrqSpinlock::new(Text::new(grid, cursor, Serial::new()))
    };
}

/// Позволяет задать цвета текста `foreground` и фона `background`.
/// При указании базовых атрибутов `base`, по умолчанию `background` берётся из них.
/// Не предполагается к непосредственному использованию вне макросов
/// [`print!()`] и [`println!()`].
#[macro_export]
macro_rules! make_attribute {
    ($foreground:expr, $background:expr) => {
        $crate::Attribute::new($foreground, $background)
    };
    ($base:expr; $foreground:expr, $background:expr) => {
        $crate::make_attribute!($foreground, $background)
    };
    ($base:expr; $foreground:expr) => {
        $crate::make_attribute!($foreground, $base.background())
    };
}

/// Печатает на экране в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
/// Аналогичен [`std::print!()`](https://doc.rust-lang.org/std/macro.print.html),
/// но дополнительно позволяет задать цвет:
/// ```ignore
/// print!(color(Color::RED), "hello");
/// print!("world");
/// ```
/// В этом случае запоминает текущий цвет до печати и восстанавливает его после.
#[macro_export]
macro_rules! print {
    (color($($colors:tt)*), $($arg:tt)*) => (
        {
            let mut text = $crate::TEXT.lock();
            let old_attribute = text.attribute();
            let new_attribute = $crate::make_attribute!(old_attribute; $($colors)*);
            text.set_attribute(new_attribute);
            if text.write_fmt(format_args!($($arg)*)).is_err() {
                text.write_str("Err").unwrap();
            }
            text.set_attribute(old_attribute);
        }
    );
    ($($arg:tt)*) => (
        $crate::TEXT.lock().write_fmt(format_args!($($arg)*)).unwrap()
    );
}

/// Печатает на экране в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
/// Аналогичен [`std::println!()`](https://doc.rust-lang.org/std/macro.println.html),
/// но дополнительно позволяет задать цвет:
/// ```ignore
/// println!(color(Color::RED), "hello");
/// println!("world");
/// ```
/// В этом случае запоминает текущий цвет до печати и восстанавливает его после.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    (color($($colors:tt)*), $($arg:tt)*) => (
        $crate::print!(
            color($($colors)*),
            "{}\n",
            format_args!($($arg)*),
        )
    );
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
