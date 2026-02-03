use core::{
    cmp,
    ops::{
        Deref,
        DerefMut,
        Range,
    },
};

use volatile::Volatile;

use serial::Serial;

use super::{
    Attribute,
    Color,
};

/// Возвращает `true`, если `octet` соответствует символу
/// [ASCII (American Standard Code for Information Interchange)](https://en.wikipedia.org/wiki/ASCII),
/// графическое представление.
pub(super) fn is_graphical(octet: u8) -> bool {
    octet != b' ' && is_printable(octet)
}

/// Возвращает `true`, если `octet` соответствует печатному символу
/// [ASCII (American Standard Code for Information Interchange)](https://en.wikipedia.org/wiki/ASCII).
pub(super) fn is_printable(octet: u8) -> bool {
    (b' ' .. 0x7F).contains(&octet)
}

// ANCHOR: glyph
/// Структура, описывающая один символ в памяти графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array)
/// при работе в текстовом режиме.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Glyph {
    /// Символ
    /// [ASCII (American Standard Code for Information Interchange)](https://en.wikipedia.org/wiki/ASCII),
    /// отображающийся в соответствующей позиции экрана.
    character: u8,

    /// Атрибуты, с которыми отображается этот символ.
    attribute: Attribute,
}
// ANCHOR_END: glyph

impl Glyph {
    /// Создаёт структуру, описывающую один символ в памяти графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array)
    /// при работе в текстовом режиме.
    pub fn new(
        character: u8,
        attribute: Attribute,
    ) -> Self {
        Self {
            character,
            attribute,
        }
    }

    /// Возвращает атрибуты, с которыми отображается этот символ.
    pub fn attribute(&self) -> Attribute {
        self.attribute
    }

    /// Возвращает символ
    /// [ASCII (American Standard Code for Information Interchange)](https://en.wikipedia.org/wiki/ASCII),
    /// отображающийся в соответствующей позиции экрана.
    pub fn character(&self) -> u8 {
        self.character
    }
}

/// Вспомогательная структура, позволяющая обернуть [`Glyph`] в [`Volatile`].
/// [`Volatile`] нужен для того, чтобы компилятор при оптимизации не выкинул
/// чтения и записи символов в память графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct GlyphWrapper(Glyph);

impl Deref for GlyphWrapper {
    type Target = Glyph;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for GlyphWrapper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Структура, описывающая содержимое экрана в памяти графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array)
/// при работе в текстовом режиме.
pub(super) type Buffer = [Volatile<GlyphWrapper>];

// ANCHOR: grid
/// Структура, позволяющая работать в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
pub struct Grid<'a> {
    /// Текущие атрибуты при печати, ---
    /// они будут использованы при печати следующего символа.
    attribute: Attribute,

    /// Содержимое экрана в памяти графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array)
    /// при работе в текстовом режиме.
    buffer: &'a mut Buffer,

    /// Номер колонки текущего положения.
    /// То есть, положения в котором находится курсор и
    /// в котором будет напечатан следующий символ.
    column: usize,

    /// Горизонтальное текстовое разрешение --- количество символов в одной строке.
    column_count: usize,

    /// Индекс в [`Grid::buffer`], с которого начинается строка символов,
    /// содержащая текущее положение.
    /// То есть, положение в котором находится курсор и
    /// в котором будет напечатан следующий символ.
    row_start: usize,

    /// Количество пробелов в символе табуляции --- `\t`.
    tab_width: usize,
}
// ANCHOR_END: grid

impl<'a> Grid<'a> {
    /// Возвращает структуру, позволяющую работать в текстовом режиме графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
    ///
    /// - Аргумент `buffer` задаёт ссылку на содержимое экрана в памяти графического контроллера
    ///   [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array)
    ///   при работе в текстовом режиме.
    /// - Текстовое разрешение равно `column`x`row_count`.
    /// - При печати табуляции --- `\t` --- будет отображено от одного до `tab_width` пробелов,
    ///   пока текущая позиция в строке не станет кратна `tab_width`.
    pub(super) fn new(
        buffer: &'a mut Buffer,
        column_count: usize,
        row_count: usize,
        tab_width: usize,
    ) -> Self {
        assert!(column_count >= 4);
        assert!(row_count >= 3);
        assert_eq!(column_count * row_count, buffer.len());

        Self {
            buffer,
            column_count,
            tab_width,
            row_start: 0,
            column: 0,
            attribute: Attribute::new(Color::GRAY, Color::BLACK),
        }
    }

    /// Возвращает текущие атрибуты при печати, ---
    /// они будут использованы при печати следующего символа.
    pub fn attribute(&mut self) -> Attribute {
        self.attribute
    }

    /// Устанавливает текущие атрибуты при печати, ---
    /// они будут использованы при печати следующего символа.
    pub fn set_attribute(
        &mut self,
        attribute: Attribute,
    ) {
        self.attribute = attribute;
    }

    /// Возвращает горизонтальное текстовое разрешение --- количество символов в одной строке.
    pub fn column_count(&self) -> usize {
        self.column_count
    }

    /// Возвращает символ в позиции `position` из памяти графического контроллера.
    pub fn glyph(
        &self,
        position: usize,
    ) -> Glyph {
        self.buffer[position].read()
    }

    /// Устанавливает символ `glyph` в позиции `position` из памяти графического контроллера.
    pub fn set_glyph(
        &mut self,
        position: usize,
        glyph: Glyph,
    ) {
        self.buffer[position].write(glyph);
    }

    /// Возвращает количестве отображаемых символов на экране.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Возвращает текущую позицию как индекс в [`Grid::buffer`].
    /// То есть, положение в котором находится курсор и
    /// в котором будет напечатан следующий символ.
    pub fn position(&self) -> usize {
        self.row_start + self.column
    }

    /// Устанавливает текущую позицию как индекс в [`Grid::buffer`].
    /// То есть, положение в котором будет напечатан следующий символ.
    pub fn set_position(
        &mut self,
        position: usize,
    ) {
        self.column = position % self.column_count();
        self.row_start = position - self.column;
    }

    /// Возвращает количество пробелов в символе табуляции --- `\t`.
    pub fn tab_width(&self) -> usize {
        self.tab_width
    }

    /// Возвращает `true`, если текущая позиция соответствует началу строки.
    pub fn is_newline(&self) -> bool {
        self.column == 0
    }

    /// Инициализирует структуру по текущему содержимому экрана.
    /// Само содержимое экрана копирует в
    /// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
    /// `serial`, игнорируя атрибуты [`Glyph::attribute`].
    /// Текущая позиция на экране задаётся позицией курсора `cursor_position`.
    pub(super) fn init<S: Serial>(
        &mut self,
        cursor_position: usize,
        serial: &mut S,
    ) -> usize {
        let position = self.printed_data_end(cursor_position);

        self.log_printed_data(serial, 0 .. position);

        if self.row_start >= self.len() {
            self.scroll();
        } else {
            self.clear(position .. self.len());
        }

        position
    }

    // ANCHOR: clear
    /// Очищает диапазон экрана, задаваемый `range`.
    /// Для этого заполняет его пробелами с текущими атрибутами [`Grid::attribute`].
    pub(super) fn clear(
        &mut self,
        range: Range<usize>,
    ) {
        // ANCHOR_END: clear
        let glyph = Glyph {
            character: b' ',
            attribute: self.attribute,
        };
        for i in range {
            self.buffer[i].write(glyph);
        }
    }

    // ANCHOR: print_character
    /// Отображает на экране символ `ch` в текущей позиции [`Grid::position()` и
    /// с текущими атрибутами [`Grid::attribute`].
    /// Специальным образом обрабатывает следующие символы:
    /// - `\t` печатает от одного до [`Grid::tab_width()`] пробелов,
    ///   пока текущая позиция в строке не станет кратна [`Grid::tab_width()`].
    /// - `\r` возвращает текущую позицию в начало строки.
    /// - `\n` переводит текущую позицию в начало следующей строки.
    pub(super) fn print_character(
        &mut self,
        ch: char,
    ) {
        // ANCHOR_END: print_character
        match ch {
            '\t' => self.tab(),
            '\r' => self.column = 0,
            '\n' => self.newline(),
            _ => self.character(ch),
        }
    }

    // ANCHOR: scroll
    /// Выполняет прокрутку содержимого экрана на одну строку вверх,
    /// и очищает нижнюю строку.
    /// Используется в ситуации, когда весь экран заполнен текстом,
    /// и требуется перейти на следующую строку.
    pub(super) fn scroll(&mut self) {
        // ANCHOR_END: scroll
        let column_count = self.column_count();
        for i in 0..self.len() - column_count {
            let glyph = self.buffer[i + column_count].read();
            self.buffer[i].write(glyph);
        }
        self.clear(self.len() - column_count..self.len());
        if self.row_start >= column_count {
            self.row_start -= column_count;
        }
    }

    // ANCHOR: adjust_position
    /// Корректирует значение [`Grid::column`] и значение [`Grid::row_start`],
    /// если первое из них вышло за пределы строки.
    /// При этом, если [`Grid::row_start`] выходит за пределы экрана,
    /// выполняет прокрутку содержимого экрана методом [`Grid::scroll()`].
    fn adjust_position(&mut self) {
        // ANCHOR_END: adjust_position
        let column_count = self.column_count();
        if self.column >= column_count {
            self.column = 0;
            self.row_start += column_count;
        }
        if self.row_start >= self.len() {
            self.scroll();
        }
    }

    // ANCHOR: character
    /// Отображает на экране символ в текущей позиции [`Grid::position()`] и
    /// с текущими атрибутами [`Grid::attribute()`].
    /// После чего продвигает текущую позицию на единицу.
    /// - Отображается `ch`, если он соответствует печатному символу
    ///   [ASCII (American Standard Code for Information Interchange)](https://en.wikipedia.org/wiki/ASCII).
    ///   См. функцию [`is_printable()`].
    /// - В противном случае отображает `?`.
    fn character(
        &mut self,
        ch: char,
    ) {
        // ANCHOR_END: character
        let display_char = if is_graphical(ch as u8) || ch as u8 == b' ' {
            ch as u8
        } else {
            b'?'
        };
        
        let glyph = Glyph {
            character: display_char,
            attribute: self.attribute,
        };
        
        let position = self.position();
        if position < self.len() {
            self.buffer[position].write(glyph);
        }
        self.column += 1;
        self.adjust_position();
    }

    // ANCHOR: newline
    /// Печатает на экране пробелы от текущей позиции [`Grid::position()`] и до конца строки.
    fn newline(&mut self) {
        // ANCHOR_END: newline
        let column_count = self.column_count();
        let position = self.position();
        let line_start = position / column_count * column_count;
        let line_end = line_start + column_count;
        self.clear(position..line_end);
        self.column = 0;
        self.row_start += column_count;
        self.adjust_position();
    }

    // ANCHOR: tab
    /// Печатает от одного до [`Grid::tab_width()`] пробелов,
    /// пока текущая позиция в строке не станет кратна [`Grid::tab_width()`].
    fn tab(&mut self) {
        let tab_width = self.tab_width();
        let column = self.column;
        let next_tab_stop = if tab_width > 0 {
            let next = ((column / tab_width) + 1) * tab_width;
            cmp::min(next, self.column_count())
        } else {
            column + 1
        };
        let glyph = Glyph {
            character: b' ',
            attribute: self.attribute,
        };
        for _ in column..next_tab_stop {
            let position = self.position();
            if position < self.len() {
                self.buffer[position].write(glyph);
            }
            self.column += 1;
            if self.column >= self.column_count() {
                self.column = 0;
                self.row_start += self.column_count();
                if self.row_start >= self.len() {
                    self.scroll();
                }
            }
        }
    }

    /// Копируется в
    /// [последовательный порт](https://en.wikipedia.org/wiki/Serial_port)
    /// `serial` содержимое экрана в диапазоне позиций `range`.
    /// Игнорирует атрибуты [`Glyph::attribute`].
    fn log_printed_data<S: Serial>(
        &mut self,
        serial: &mut S,
        range: Range<usize>,
    ) {
        self.row_start = 0;
        self.column = 0;

        let mut prev_graphical = 0;

        for i in range {
            if is_graphical(self.buffer[i].read().character) {
                while prev_graphical <= i {
                    serial.print_octet(self.buffer[prev_graphical].read().character);
                    prev_graphical += 1;
                }
            }

            self.column += 1;

            if self.column >= self.column_count() {
                self.row_start += self.column_count();
                self.column = 0;
                prev_graphical = i + 1;
                serial.print_octet(b'\n');
            }
        }
    }

    /// Вычисляет текущую позицию на экране как максимальную из:
    /// - Текущей позиции курсора `cursor_position`.
    /// - И максимальной позицией, перед которой отображён печатный символов.
    fn printed_data_end(
        &self,
        cursor_position: usize,
    ) -> usize {
        let position = cmp::min(cursor_position, self.len());
        let mut printed_data_end = position;

        for i in position .. self.len() {
            if is_graphical(self.buffer[i].read().character) {
                printed_data_end = i + 1;
            }
        }

        printed_data_end
    }
}
