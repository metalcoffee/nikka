use core::cmp;

use ku::memory::{
    IndexDataPair,
    IndexDataPortPair,
};

// Used in docs.
#[allow(unused)]
use super::Grid;

/// Типаж для управления курсором в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
pub trait Cursor {
    #[allow(rustdoc::private_intra_doc_links)]
    // ANCHOR: get
    /// Возвращает позицию курсора.
    ///
    /// Эта позиция соответствует индексу в массиве [`Grid`].
    /// Так же как и этот массив, считается в позициях текстового режима с нуля,
    /// из левого верхнего угла по направлению слева направо.
    /// При достижении правой границы осуществляется переход в самую левую позицию следующей строки.
    fn get(&mut self) -> usize;
    // ANCHOR_END: get

    #[allow(rustdoc::private_intra_doc_links)]
    // ANCHOR: set
    /// Устанавливает позицию курсора.
    ///
    /// Эта позиция соответствует индексу в массиве [`Grid`].
    /// Так же как и этот массив, считается в позициях текстового режима с нуля,
    /// из левого верхнего угла по направлению слева направо.
    /// При достижении правой границы осуществляется переход в самую левую позицию следующей строки.
    fn set(
        &mut self,
        position: usize,
    );
    // ANCHOR_END: set

    // ANCHOR: set_disable
    /// Включает или отключает отображение курсора.
    fn set_disable(
        &mut self,
        disable: bool,
    );
    // ANCHOR_END: set_disable

    #[allow(rustdoc::private_intra_doc_links)]
    // ANCHOR: set_height
    /// Устанавливает размер курсора.
    /// Задавать можно только высоту курсора от `0` до [`MAX_LINES`] включительно.
    fn set_height(
        &mut self,
        height: u8,
    );
    // ANCHOR_END: set_height
}

/// Структура для управления курсором в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
///
/// Для тестов может быть создана поверх эмулируемых
/// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O),
/// см. [`IndexDataPair`].
pub struct VgaCursor<T: IndexDataPair<u8>>(T);

impl<T: IndexDataPair<u8>> VgaCursor<T> {
    /// Создаёт структуру для управления курсором в текстовом режиме графического контроллера
    /// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
    /// Аргумент `port_pair` пару индекс--данные
    /// [портов ввода--вывода](https://en.wikipedia.org/wiki/Memory-mapped_I/O_and_port-mapped_I/O).
    ///
    /// Для создания настоящего VGA-курсора предназначена дополнительная функция
    /// [`create_vga_cursor()`],
    /// которая вызывает [`VgaCursor::new()`] с аргументом, задающим правильные номера портов.
    pub(super) unsafe fn new(port_pair: T) -> Self {
        Self(port_pair)
    }

}

impl<T: IndexDataPair<u8>> Cursor for VgaCursor<T> {
    fn get(&mut self) -> usize {
        let high = unsafe { self.0.read(POSITION_HIGH) };
        let low = unsafe { self.0.read(POSITION_LOW) };
        ((high as usize) << 8) | (low as usize)
    }

    fn set(
        &mut self,
        position: usize,
    ) {
        let high = (position >> 8) as u8;
        let low = position as u8;
        unsafe {
            self.0.write(POSITION_HIGH, high);
            self.0.write(POSITION_LOW, low);
        }
    }

    fn set_disable(
        &mut self,
        disable: bool,
    ) {
        let start_line = unsafe { self.0.read(START_LINE) };
        let new_start_line = if disable {
            start_line | CURSOR_DISABLE
        } else {
            start_line & !CURSOR_DISABLE
        };
        unsafe {
            self.0.write(START_LINE, new_start_line);
        }
    }

    fn set_height(
        &mut self,
        height: u8,
    ) {
        let end_line = MAX_LINES - 1;
        let start_line = (end_line + 1).saturating_sub(height);
        let current_start = unsafe { self.0.read(START_LINE) };
        let disable_bit = current_start & CURSOR_DISABLE;
        let new_start_line = (start_line & CURSOR_LINE_MASK) | disable_bit;
        
        unsafe {
            self.0.write(START_LINE, new_start_line);
            self.0.write(END_LINE, end_line);
        }
    }
}

/// Создаёт структуру для управления курсором в текстовом режиме графического контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
pub(super) fn create_vga_cursor() -> VgaCursor<IndexDataPortPair<u8>> {
    const INDEX_PORT: u16 = 0x03D4;

    unsafe {
        VgaCursor::new(
            IndexDataPortPair::from_index_port(INDEX_PORT).expect("invalid VGA cursor index port"),
        )
    }
}

/// Вес бита выключения курсора в регистре [`START_LINE`] контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
pub(super) const CURSOR_DISABLE: u8 = 1 << 5;

/// Маска битов начальной и конечной линий курсора в регистрах [`START_LINE`] и [`END_LINE`]
/// контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array).
pub(super) const CURSOR_LINE_MASK: u8 = (1 << 5) - 1;

/// Индекс регистра контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array),
/// хранящий начальную линию текстового курсора.
/// Вместе с [`END_LINE`] задаёт размер курсора.
pub(super) const START_LINE: u8 = 0x0A;

/// Индекс регистра контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array),
/// хранящий начальную линию текстового курсора.
/// Вместе с [`START_LINE`] задаёт размер курсора.
pub(super) const END_LINE: u8 = 0x0B;

/// Максимальное количество горизонтальных линий текстового курсора контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array),
pub(super) const MAX_LINES: u8 = 1 << 4;

/// Индекс регистра контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array),
/// хранящего старший байт позиции текстового курсора.
/// Вместе с [`POSITION_LOW`] задаёт положение курсора на экране.
pub(super) const POSITION_HIGH: u8 = 0x0E;

/// Индекс регистра контроллера
/// [Video Graphics Array (VGA)](https://en.wikipedia.org/wiki/Video_Graphics_Array),
/// хранящего младший байт позиции курсора на экране.
/// Вместе с [`POSITION_HIGH`] задаёт положение курсора на экране.
pub(super) const POSITION_LOW: u8 = 0x0F;
