use core::{
    cell::Cell,
    cmp,
    fmt::{
        Debug,
        Display,
        Formatter,
        Result,
        Write,
    },
    mem,
};

use tracing_core::LevelFilter;
use tracing_subscriber::{
    self,
    EnvFilter,
    fmt,
};
use volatile::Volatile;

use ku::memory::{
    IndexDataPair,
    size,
};

use serial::Serial;

use super::{
    Text,
    cursor::{
        self,
        Cursor,
        VgaCursor,
    },
    grid::{
        Buffer,
        GlyphWrapper,
        Grid,
    },
};

const COLUMN_COUNT: usize = 80;
const ROW_COUNT: usize = 50;
const LEN: usize = COLUMN_COUNT * ROW_COUNT;

const TAB_WIDTH: usize = 8;

fn mock_buffer() -> [Volatile<GlyphWrapper>; LEN] {
    unsafe { mem::zeroed() }
}

fn mock_grid(
    buffer: &mut Buffer,
    column_count: usize,
    row_count: usize,
    tab_width: usize,
) -> Grid<'_> {
    Grid::new(buffer, column_count, row_count, tab_width)
}

#[test]
fn out_of_bounds() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);

                let chars = ['\t', '\n', '\r', '*'];

                for ch in chars.iter() {
                    for _ in 0 .. 2 * len {
                        grid.print_character(*ch);
                    }
                }
            }
        }
    }
}

#[test]
fn out_of_bounds_on_init() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);
                let mut serial = MockSerial::new();

                grid.init(0, &mut serial);

                for _ in 0 .. len - 1 {
                    grid.print_character('*');
                }

                grid.init(len - 1, &mut serial);

                grid.print_character('*');
            }
        }
    }
}

#[test]
fn out_of_bounds_on_tab() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                if column_count < 2 * tab_width {
                    continue;
                }

                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);

                while grid.position() < len - grid.column_count() {
                    grid.print_character('\n');
                }

                for tabs_position in grid.column_count() - 2 * grid.tab_width() ..
                    grid.column_count() + 2 * grid.tab_width()
                {
                    grid.print_character('\n');

                    assert_position(
                        &grid,
                        len - grid.column_count(),
                        "That is, at the last line of the Grid.\n",
                    );

                    for _ in 0 .. tabs_position {
                        grid.print_character('*');
                    }
                    for _ in tabs_position .. grid.column_count() + 2 * grid.tab_width() {
                        grid.print_character('\t');
                    }
                }
            }
        }
    }
}

#[test]
fn scroll() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut before_scroll = mock_buffer();
                let position;

                {
                    let mut grid = mock_grid(
                        &mut before_scroll[.. len],
                        column_count,
                        row_count,
                        tab_width,
                    );
                    grid.init(0, &mut MockSerial::new());

                    for ch in Filler::new(grid.len() - 1) {
                        grid.print_character(ch);
                    }

                    position = grid.position();
                    assert!(
                        position > grid.len() - grid.column_count(),
                        concat!(
                            "\n\nExpected that all the lines of the Grid are filled with some \
                             text.\n",
                            "But the current position in the Grid is the {:?}.\n",
                            "Whilst the last line begins from the {:?}.\n\n",
                        ),
                        format::position(grid.column_count(), position),
                        format::position(grid.column_count(), grid.len() - grid.column_count()),
                    );
                }

                let mut after_scroll = mock_buffer();
                for i in 0 .. len {
                    after_scroll[i].write(before_scroll[i].read());
                }

                {
                    let mut grid = mock_grid(
                        &mut after_scroll[.. len],
                        column_count,
                        row_count,
                        tab_width,
                    );
                    grid.init(position, &mut MockSerial::new());
                    grid.scroll();
                }

                for new_position in 0 .. len - column_count {
                    let old_position = new_position + column_count;
                    let expected = before_scroll[old_position].read();
                    let found = after_scroll[new_position].read();
                    assert_eq!(
                        found,
                        expected,
                        concat!(
                            "\n\nExpected that the {:?}\n",
                            "will move one row up from the {:?}\n",
                            "to the {:?} after the Grid::scroll().\n",
                            "But found the {:?} there.\n",
                            "The buffer before the scroll:\n{:?}\n",
                            "The buffer after the scroll:\n{:?}\n\n",
                        ),
                        expected,
                        format::position(column_count, old_position),
                        format::position(column_count, new_position),
                        found,
                        format::buffer(&before_scroll, column_count, row_count),
                        format::buffer(&after_scroll, column_count, row_count),
                    );
                }

                for position in len - column_count .. len {
                    let found = after_scroll[position].read().character();
                    assert_eq!(
                        found,
                        b' ',
                        concat!(
                            "\n\nExpected that the last row will be filled\n",
                            "with the white space after the Grid::scroll().\n",
                            "But found the {:?}\n",
                            "at the {:?}.\n",
                            "The buffer is:\n{:?}\n\n",
                        ),
                        format::character(found),
                        format::position(column_count, position),
                        format::buffer(&after_scroll, column_count, row_count),
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_newline() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);

                grid.print_character('\n');

                assert_position(
                    &grid,
                    grid.column_count(),
                    "After printing a single '\\n' character.\n",
                );

                grid.print_character('\n');

                assert_position(
                    &grid,
                    2 * grid.column_count(),
                    "After printing two '\\n' characters.\n",
                );
            }
        }
    }
}

#[test]
fn explicit_newline_after_an_implicit_one() {
    for column_count in 4 ..= 20 {
        for row_count in 4 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);

                fill_line(&mut grid, '*');

                assert_position(
                    &grid,
                    grid.column_count(),
                    "After printing a full line - #column_count of non-control characters.\n",
                );

                grid.print_character('\n');

                assert_position(
                    &grid,
                    2 * grid.column_count(),
                    "After printing a full line - #column_count of non-control characters - \
                     followed by a '\\n'.\n",
                );

                grid.print_character('\n');

                assert_position(
                    &grid,
                    3 * grid.column_count(),
                    "After printing a full line - #column_count of non-control characters - \
                     followed by '\\n\\n'.\n",
                );
            }
        }
    }
}

#[test]
fn position_is_always_less_than_size() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);

                for _ in 0 .. len - 1 {
                    grid.print_character('*');
                }

                assert_position(
                    &grid,
                    len - 1,
                    "After printing #(len - 1) of non-control characters.\n",
                );

                grid.print_character('*');

                assert_position(
                    &grid,
                    len - grid.column_count(),
                    concat!(
                        "After filling the full screen with #len of non-control characters.\n",
                        "That is, expected that the Grid::scroll() will be triggered once.\n",
                    ),
                );
            }
        }
    }
}

#[test]
fn carriage_return() {
    for column_count in 4 ..= 20 {
        for row_count in 3 ..= 10 {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let mut buffer = mock_buffer();
                let mut grid = mock_grid(&mut buffer[.. len], column_count, row_count, tab_width);

                fill(&mut grid, '*', column_count / 2);

                assert_position(
                    &grid,
                    grid.column_count() / 2,
                    "After printing a half of a line - of #(column_count / 2) non-control \
                     characters.\n",
                );

                grid.print_character('\r');

                assert_position(
                    &grid,
                    0,
                    "After printing a half of a line - of #(column_count / 2) non-control \
                     characters - followed by a '\\r'.\n",
                );

                fill_line(&mut grid, '#');

                assert_position(
                    &grid,
                    grid.column_count(),
                    "After printing a full line - of #column_count non-control characters.\n",
                );

                grid.print_character('\r');

                assert_position(
                    &grid,
                    grid.column_count(),
                    "After printing a full line - of #column_count non-control characters - \
                     followed by a '\\r'.\n",
                );

                assert_eq!(
                    buffer[0].read().character(),
                    b'#',
                    concat!(
                        "\n\nExpected that the first character in the buffer will be the '#'.\n",
                        "After printing a half of a line of '*', then a '\\r', and then a full \
                         line of the '#'.\n",
                        "But found it to be the {:?}.\n\n",
                    ),
                    format::character(buffer[0].read().character()),
                );
            }
        }
    }
}

fn fill_line(
    grid: &mut Grid,
    ch: char,
) {
    fill(grid, ch, grid.column_count());
}

fn fill(
    grid: &mut Grid,
    ch: char,
    count: usize,
) {
    for _ in 0 .. count {
        grid.print_character(ch);
    }
}

fn assert_position(
    grid: &Grid,
    position: usize,
    message: &str,
) {
    assert_eq!(
        grid.position(),
        position,
        concat!(
            "\n\nExpected that the position will be the {:?}.\n",
            "{}",
            "But found it to be the {:?}.\n\n",
        ),
        format::position(grid.column_count(), position),
        message,
        format::position(grid.column_count(), grid.position()),
    );
}

#[test]
fn printing() {
    for column_count in COLUMN_COUNT / 2 ..= COLUMN_COUNT {
        for row_count in ROW_COUNT / 2 ..= ROW_COUNT {
            for tab_width in 1 ..= TAB_WIDTH {
                let len = column_count * row_count;

                let test_data = [
                    Filler::new(2 * column_count).start('0'),
                    Filler::new(2 * column_count - 1).start('a').step(3),
                    Filler::new(2 * column_count + 1).start('A').step(5),
                    Filler::new(2 * column_count - 2).start(' ').step(7),
                    Filler::new(2 * column_count + 2).start(':').step(11),
                    Filler::new(4 * column_count).start('=').step(13).enable_tabs(),
                ];

                let mut buffer = mock_buffer();

                let mut cursor_position = 0;
                let mut restore_cursor_position = false;

                for (part_index, part) in test_data.iter().enumerate() {
                    {
                        let grid =
                            Grid::new(&mut buffer[.. len], column_count, row_count, tab_width);
                        let cursor = MockCursor::new();
                        let mut text = Text::new(grid, cursor.get(), MockSerial::new());

                        if restore_cursor_position {
                            cursor.get().set(cursor_position);
                            restore_cursor_position = !restore_cursor_position;
                        }

                        text.init();

                        write!(text, "{part}").unwrap();

                        cursor_position = cursor.get().get();
                    }

                    assert!(
                        cursor_position + column_count < len,
                        concat!(
                            "\n\nDecrease the total length of the test data\n",
                            "or the number of lines in it.\n",
                            "This test requires that the Grid::scroll() is not triggered.\n",
                            "The cursor is now at the {:?}.\n",
                            "It is the last row of the buffer which length is {}x{} = {}.\n",
                            "That means the Grid::scroll() could have been triggered already.\n\n",
                        ),
                        format::position(column_count, cursor_position),
                        column_count,
                        row_count,
                        len,
                    );

                    check(
                        &buffer,
                        column_count,
                        row_count,
                        tab_width,
                        cursor_position,
                        &test_data[.. part_index + 1],
                    );
                }
            }
        }
    }
}

fn check(
    buffer: &Buffer,
    column_count: usize,
    row_count: usize,
    tab_width: usize,
    cursor_position: usize,
    data: &[Filler],
) {
    let len = column_count * row_count;
    let mut position = 0;

    for part in data {
        if position % column_count != 0 {
            position = check_white_space(
                buffer,
                column_count,
                row_count,
                position,
                column_count,
                data,
            );
        }
        if position != 0 {
            position = check_white_space(
                buffer,
                column_count,
                row_count,
                position,
                column_count,
                data,
            );
        }

        let mut implicit_newline = false;

        for ch in *part {
            assert!(
                position < len,
                concat!(
                    "\n\nThe data exceeded the buffer of length {}\n",
                    "at the {:?}.\n",
                    "The data is: {:?}.\n",
                    "The buffer is:\n{:?}\n\n",
                ),
                len,
                format::position(column_count, position),
                data,
                format::buffer(buffer, column_count, row_count),
            );

            match ch {
                '\t' => {
                    position = check_white_space(
                        buffer,
                        column_count,
                        row_count,
                        position,
                        tab_width,
                        data,
                    );
                    implicit_newline = position % column_count == 0;
                },
                '\n' =>
                    if implicit_newline {
                        implicit_newline = false;
                    } else {
                        position = check_white_space(
                            buffer,
                            column_count,
                            row_count,
                            position,
                            column_count,
                            data,
                        );
                    },
                _ => {
                    let expected = ch as u8;
                    let found = buffer[position].read().character();
                    assert_eq!(
                        found,
                        expected,
                        concat!(
                            "\n\nExpected the {:?}\n",
                            "but found the {:?}\n",
                            "at the {:?}.\n",
                            "Tab width is: {:?}.\n",
                            "The data is: {:?}.\n",
                            "The buffer is:\n{:?}\n\n",
                        ),
                        format::character(expected),
                        format::character(found),
                        format::position(column_count, position),
                        tab_width,
                        data,
                        format::buffer(buffer, column_count, row_count),
                    );
                    position += 1;

                    implicit_newline = position % column_count == 0;
                },
            }
        }
    }

    assert_eq!(
        cursor_position,
        position,
        concat!(
            "\n\nExpected the cursor to be at the {:?}\n",
            "but found it at the {:?}.\n\n",
        ),
        format::position(column_count, position),
        format::position(column_count, cursor_position),
    );
}

fn check_white_space(
    buffer: &Buffer,
    column_count: usize,
    row_count: usize,
    begin: usize,
    alignment: usize,
    data: &[Filler],
) -> usize {
    let end = cmp::min(
        begin / column_count * column_count +
            cmp::min(
                (begin % column_count + 1).next_multiple_of(alignment),
                (begin % column_count + 1).next_multiple_of(column_count),
            ),
        column_count * row_count,
    );

    for position in begin .. end {
        let found = buffer[position].read().character();
        assert_eq!(
            found,
            b' ',
            concat!(
                "\n\nExpected the white space from the {:?}\n",
                "until the {}-cells aligned {:?}.\n",
                "But found the {:?}\n",
                "at the {:?}.\n",
                "The data is: {:?}.\n",
                "The buffer is:\n{:?}\n\n",
            ),
            format::position(column_count, begin),
            alignment,
            format::position(column_count, end),
            format::character(found),
            format::position(column_count, position),
            data,
            format::buffer(buffer, column_count, row_count),
        );
    }

    end
}

#[test]
fn cursor() {
    let cursor = MockCursor::new();

    for height in 0 .. cursor::MAX_LINES {
        cursor.get().set_height(height);

        let ports = cursor.ports.get();
        assert_eq!(ports.end_line, cursor::MAX_LINES - 1);
        assert_eq!(ports.end_line + 1 - ports.begin_line, height);

        cursor.get().set_disable(true);

        let ports = cursor.ports.get();
        assert_eq!(ports.end_line, cursor::MAX_LINES - 1);
        assert_eq!(
            ports.end_line + 1 - (ports.begin_line & cursor::CURSOR_LINE_MASK),
            height,
        );
        assert_eq!(
            ports.begin_line & !cursor::CURSOR_LINE_MASK,
            cursor::CURSOR_DISABLE,
        );

        cursor.get().set_disable(false);

        let ports = cursor.ports.get();
        assert_eq!(ports.end_line, cursor::MAX_LINES - 1);
        assert_eq!(ports.end_line + 1 - ports.begin_line, height);
    }

    for position in 0 .. COLUMN_COUNT * ROW_COUNT {
        cursor.get().set(position);
        assert_eq!(cursor.ports.get().position(), position);
        assert_eq!(cursor.get().get(), position);
    }
}

#[derive(Clone, Copy, Default)]
struct MockCursorPorts {
    begin_line: u8,
    end_line: u8,
    position_high: u8,
    position_low: u8,
}

impl MockCursorPorts {
    fn port(
        &mut self,
        index: u8,
    ) -> &mut u8 {
        match index {
            cursor::START_LINE => &mut self.begin_line,
            cursor::END_LINE => &mut self.end_line,
            cursor::POSITION_HIGH => &mut self.position_high,
            cursor::POSITION_LOW => &mut self.position_low,
            _ => panic!("wrong VGA cursor port used"),
        }
    }

    fn position(&self) -> usize {
        (size::from(self.position_high) << 8) | size::from(self.position_low)
    }
}

struct MockCursor {
    ports: Cell<MockCursorPorts>,
}

impl IndexDataPair<u8> for &MockCursor {
    unsafe fn read(
        &mut self,
        index: u8,
    ) -> u8 {
        *self.ports.get().port(index)
    }

    unsafe fn write(
        &mut self,
        index: u8,
        data: u8,
    ) {
        self.ports.update(|mut ports| {
            *ports.port(index) = data;
            ports
        });
    }
}

impl MockCursor {
    fn new() -> Self {
        Self {
            ports: Cell::new(MockCursorPorts::default()),
        }
    }

    fn get(&self) -> VgaCursor<&MockCursor> {
        unsafe { VgaCursor::new(self) }
    }
}

struct MockSerial {}

impl Serial for MockSerial {
    fn new() -> Self {
        Self {}
    }

    fn print_octet(
        &mut self,
        _: u8,
    ) {
    }
}

#[derive(Clone, Copy)]
struct Filler {
    current_octet: u8,
    remaining_len: usize,
    until_tab: usize,
    next_until_tab: usize,
    step: u8,
}

impl Filler {
    const PRIME: u8 = 89;
    const BEGIN: u8 = b' ';
    const END: u8 = Self::BEGIN + Self::PRIME;

    fn new(len: usize) -> Self {
        Self {
            current_octet: Self::BEGIN,
            remaining_len: len,
            until_tab: len,
            next_until_tab: 0,
            step: 1,
        }
    }

    fn start(
        mut self,
        start: char,
    ) -> Self {
        self.current_octet = start as u8;

        self
    }

    fn step(
        mut self,
        step: u8,
    ) -> Self {
        self.step = step;

        self
    }

    fn enable_tabs(mut self) -> Self {
        self.schedule_next_tab();

        self
    }

    fn schedule_next_tab(&mut self) {
        const MAX_UNTIL_TAB: usize = 17;

        self.until_tab = self.next_until_tab % MAX_UNTIL_TAB;
        self.next_until_tab = self.until_tab + 1;
    }
}

impl Iterator for Filler {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_len == 0 {
            return None;
        }

        self.remaining_len -= 1;

        if self.until_tab == 0 {
            self.schedule_next_tab();

            Some('\t')
        } else {
            self.until_tab -= 1;

            self.current_octet = self.current_octet.wrapping_add(self.step);
            if self.current_octet >= Self::END {
                self.current_octet = Self::BEGIN;
            }

            let current_octet = if self.remaining_len > 1 || self.current_octet != b' ' {
                self.current_octet as char
            } else {
                '*'
            };

            Some(current_octet)
        }
    }
}

impl Debug for Filler {
    fn fmt(
        &self,
        formatter: &mut Formatter,
    ) -> Result {
        write!(formatter, "\"")?;

        for ch in *self {
            match ch {
                '\t' => write!(formatter, "\\t")?,
                '\r' => write!(formatter, "\\r")?,
                '\n' => write!(formatter, "\\n")?,
                '\\' | '"' => write!(formatter, "\\{ch}")?,
                _ => write!(formatter, "{ch}")?,
            }
        }

        write!(formatter, "\"")
    }
}

impl Display for Filler {
    fn fmt(
        &self,
        formatter: &mut Formatter,
    ) -> Result {
        for ch in *self {
            write!(formatter, "{ch}")?;
        }

        Ok(())
    }
}

mod format {
    use core::{
        cmp,
        fmt::{
            Debug,
            Formatter,
            Result,
        },
    };

    use super::super::grid::{
        Buffer,
        is_graphical,
    };

    pub struct BufferFormatter<'a> {
        buffer: &'a Buffer,
        column_count: usize,
        row_count: usize,
    }

    impl<'a> BufferFormatter<'a> {
        fn len(&self) -> usize {
            self.column_count * self.row_count
        }
    }

    pub fn buffer(
        buffer: &Buffer,
        column_count: usize,
        row_count: usize,
    ) -> BufferFormatter<'_> {
        BufferFormatter {
            buffer,
            column_count,
            row_count,
        }
    }

    impl<'a> Debug for BufferFormatter<'a> {
        fn fmt(
            &self,
            formatter: &mut Formatter,
        ) -> Result {
            format_header(formatter, 2, self.column_count, 10, 5)?;
            writeln!(formatter)?;

            let mut end = self.len();
            while end >= 1 && self.buffer[end - 1].read().character() == b'\0' {
                end -= 1;
            }
            end = cmp::min(end.next_multiple_of(self.column_count), self.len());

            for (index, glyph) in self.buffer[0 .. end].iter().enumerate() {
                let mut ch = glyph.read().character();
                if !is_graphical(ch) {
                    ch = b' ';
                }

                let position = position(self.column_count, index);
                if position.column == 0 {
                    if position.row > 0 {
                        writeln!(formatter, "│")?;
                    }
                    write!(formatter, "{:2}│", position.row)?;
                }

                write!(formatter, "{}", ch as char)?;
            }

            writeln!(formatter, "│")?;

            format_footer(formatter, 2, self.column_count, 10, 5)
        }
    }

    fn format_labels(
        formatter: &mut Formatter,
        indent: usize,
        width: usize,
        label_distance: usize,
    ) -> Result {
        assert!(
            label_distance >= 3,
            "\n\nlabel_distance must be at least 3, otherwise the labels will not fit.\n\n",
        );

        for _ in 0 .. indent + 1 {
            write!(formatter, " ")?;
        }

        for i in 0 .. width {
            if i % label_distance == 0 {
                write!(formatter, "{i:<2}")?;
            } else if i % label_distance != 1 {
                write!(formatter, " ")?;
            }
        }

        Ok(())
    }

    struct LineChars {
        line: char,
        mark: char,
        left_corner: char,
        right_corner: char,
    }

    fn format_line(
        formatter: &mut Formatter,
        indent: usize,
        width: usize,
        mark_distance: usize,
        chars: &LineChars,
    ) -> Result {
        for _ in 0 .. indent {
            write!(formatter, " ")?;
        }

        write!(formatter, "{}", chars.left_corner)?;

        for i in 0 .. width {
            if mark_distance > 0 && i % mark_distance == 0 {
                write!(formatter, "{}", chars.mark)?;
            } else {
                write!(formatter, "{}", chars.line)?;
            }
        }

        write!(formatter, "{}", chars.right_corner)
    }

    fn format_header(
        formatter: &mut Formatter,
        indent: usize,
        width: usize,
        label_distance: usize,
        mark_distance: usize,
    ) -> Result {
        if label_distance > 0 {
            format_labels(formatter, indent, width, label_distance)?;
            writeln!(formatter)?;
        }

        const HEADER_CHARS: LineChars = LineChars {
            line: '─',
            mark: '┴',
            left_corner: '┌',
            right_corner: '┐',
        };

        format_line(formatter, indent, width, mark_distance, &HEADER_CHARS)
    }

    fn format_footer(
        formatter: &mut Formatter,
        indent: usize,
        width: usize,
        label_distance: usize,
        mark_distance: usize,
    ) -> Result {
        const FOOTER_CHARS: LineChars = LineChars {
            line: '─',
            mark: '┬',
            left_corner: '└',
            right_corner: '┘',
        };

        format_line(formatter, indent, width, mark_distance, &FOOTER_CHARS)?;

        if label_distance > 0 {
            writeln!(formatter)?;
            format_labels(formatter, indent, width, label_distance)
        } else {
            Ok(())
        }
    }

    #[derive(Debug)]
    pub struct Character {
        #[allow(unused)]
        value: char,
        #[allow(unused)]
        code: u8,
    }

    pub fn character(code: u8) -> Character {
        Character {
            value: code as char,
            code,
        }
    }

    #[derive(Debug)]
    pub struct Position {
        row: usize,
        column: usize,
        #[allow(unused)]
        index: usize,
    }

    pub fn position(
        column_count: usize,
        index: usize,
    ) -> Position {
        Position {
            row: index / column_count,
            column: index % column_count,
            index,
        }
    }
}

#[ctor::ctor]
fn init() {
    let filter = EnvFilter::from_default_env().add_directive(LevelFilter::DEBUG.into());

    let format = fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(true)
        .compact();

    tracing_subscriber::fmt()
        .with_ansi(false)
        .event_format(format)
        .with_env_filter(filter)
        .init();
}
