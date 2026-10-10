//! The GW-BASIC screen. There is no separate line buffer: the cells on the
//! screen are the line. A logical line is one or more physical rows joined by
//! a continuation flag, at most 255 characters.
//!
//! Graphics modes keep an attribute framebuffer beside that text grid. The
//! window composites the text over the pixels; `POINT` reads the buffer.

pub const SCREEN_ROWS: usize = 25;
pub const MAX_LOGICAL: usize = 255;
pub const ATTR_NORMAL: u8 = 0x07;

/// The 16-colour EGA palette, also used for text attributes.
pub const EGA16: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555,
    0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// `SCREEN 9` default attribute registers. Index 20 is brown, so attribute 6
/// stays brown instead of becoming yellow.
const EGA64_DEFAULT: [u16; 16] = [0, 1, 2, 3, 4, 5, 20, 7, 56, 57, 58, 59, 60, 61, 62, 63];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxErr {
    Illegal,
    Overflow,
}

/// Arguments of `SCREEN [mode][,[burst][,[apage][,[vpage]]]]`.
/// `erase` is the PCjr/Tandy fifth argument, which this machine rejects.
#[derive(Clone, Copy, Debug)]
pub struct ScreenArgs {
    pub mode: Option<i16>,
    pub burst: Option<i16>,
    pub active: Option<i16>,
    pub visible: Option<i16>,
    pub erase: bool,
}

/// `COLOR` in a graphics mode. Omitted arguments are `None`.
#[derive(Clone, Copy, Debug)]
pub struct ColorArgs {
    pub foreground: Option<i16>,
    pub palette_0: Option<i16>,
    pub extra: Option<i16>,
}

/// What `PAINT` writes into each filled pixel.
#[derive(Clone, Debug)]
pub enum Fill {
    Solid(u8),
    /// Tile bytes, one row per byte, repeating down the screen. At most 64 bytes.
    Tile(Vec<u8>),
}

/// `CIRCLE` with the angles still carrying their signs. A negative angle draws
/// a radius from the centre to that end of the arc.
#[derive(Clone, Copy, Debug)]
pub struct Circle {
    pub cx: f64,
    pub cy: f64,
    pub radius: f64,
    pub attr: u8,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub aspect: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: u8,
    pub attr: u8,
}

#[derive(Clone, Debug)]
struct Row {
    cells: Vec<Cell>,
    /// The next physical row is part of this logical line (wrap or Ctrl+Enter).
    continues: bool,
}

pub struct Screen {
    width: usize,
    rows: Vec<Row>,
    cursor_row: usize,
    cursor_col: usize,
    insert: bool,
    keys_on: bool,
    attr: u8,
    gfx: Gfx,
}

impl Screen {
    pub fn new(width: usize) -> Self {
        let width = if width == 40 { 40 } else { 80 };
        let mut screen = Self {
            width,
            rows: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            insert: false,
            keys_on: true,
            attr: ATTR_NORMAL,
            gfx: Gfx::text(),
        };
        screen.rows = (0..SCREEN_ROWS).map(|_| screen.blank_row()).collect();
        screen
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row;
        self.cursor_col = col;
        self.clamp_cursor();
    }

    pub fn insert_mode(&self) -> bool {
        self.insert
    }

    pub fn set_insert(&mut self, on: bool) {
        self.insert = on;
    }

    pub fn attr(&self) -> u8 {
        self.attr
    }

    /// Foreground is the low nibble and background is the high nibble.
    pub fn set_attr(&mut self, attr: u8) {
        self.attr = attr;
    }

    pub fn keys_visible(&self) -> bool {
        self.keys_on
    }

    pub fn set_keys_on(&mut self, on: bool) {
        self.keys_on = on;
        self.clamp_cursor();
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.rows[row].cells[col]
    }

    pub fn continues(&self, row: usize) -> bool {
        self.rows[row].continues
    }

    pub fn editable_rows(&self) -> usize {
        if self.keys_on {
            SCREEN_ROWS - 1
        } else {
            SCREEN_ROWS
        }
    }

    pub fn clear(&mut self) {
        self.clear_text();
        if self.gfx.mode != 0 {
            self.gfx.pixels.fill(0);
            self.gfx.reset_pen();
        }
    }

    fn clear_text(&mut self) {
        let blank = self.blank_row();
        let editable = self.editable_rows();
        for row in &mut self.rows[..editable] {
            *row = blank.clone();
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub fn home(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub fn set_width(&mut self, width: usize) -> bool {
        if width != 40 && width != 80 {
            return false;
        }
        if self.gfx.mode != 0 {
            return self.set_graphics_width(width);
        }
        self.width = width;
        self.rows = (0..SCREEN_ROWS).map(|_| self.blank_row()).collect();
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.insert = false;
        true
    }

    pub fn logical_text(&self) -> String {
        let (start, end) = self.span_at(self.cursor_row);
        bytes_to_string(&self.read_stripped(start, end))
    }

    pub fn snapshot(&self) -> String {
        let mut lines = Vec::new();
        for r in 0..self.editable_rows() {
            let text: String = self.rows[r]
                .cells
                .iter()
                .map(|c| char::from(c.ch))
                .collect();
            lines.push(text.trim_end().to_string());
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    pub fn print_str(&mut self, s: &str) {
        let mut buf = Vec::with_capacity(s.len());
        for c in s.chars() {
            if c == '\n' {
                buf.push(b'\n');
            } else if c == '\r' {
                buf.push(b'\r');
            } else if c == '\u{2190}' {
                buf.push(0x1B);
            } else if (c as u32) <= 0xFF {
                buf.push(c as u8);
            }
        }
        self.print_bytes(&buf);
    }

    pub fn print_bytes(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                        i += 1;
                    }
                    self.newline();
                }
                b'\n' => self.newline(),
                0x07 => {}
                ch if ch >= 0x20 || ch == 0x1B => self.put_print_char(ch),
                _ => {}
            }
            i += 1;
        }
    }

    pub fn type_char(&mut self, ch: u8) {
        if ch < 0x20 && ch != 0x1B {
            return;
        }
        let (start, end, mut chars, idx) = self.content_and_cursor();
        if self.insert {
            if chars.len() >= MAX_LOGICAL {
                return;
            }
            chars.insert(idx, ch);
            self.splice_logical(start, end, &chars, idx + 1);
        } else if idx >= chars.len() {
            if chars.len() >= MAX_LOGICAL {
                return;
            }
            chars.push(ch);
            let pos = chars.len();
            self.splice_logical(start, end, &chars, pos);
        } else {
            chars[idx] = ch;
            self.splice_logical(start, end, &chars, idx + 1);
        }
    }

    pub fn backspace(&mut self) {
        // At the left edge, Backspace is Delete (User's Guide chapter 4).
        if self.cursor_col == 0 {
            self.delete_char();
            return;
        }
        let (start, end, mut chars, idx) = self.content_and_cursor();
        if idx == 0 || chars.is_empty() {
            return;
        }
        chars.remove(idx - 1);
        self.splice_logical(start, end, &chars, idx - 1);
    }

    pub fn delete_char(&mut self) {
        let (start, end, mut chars, idx) = self.content_and_cursor();
        if idx < chars.len() {
            chars.remove(idx);
            self.splice_logical(start, end, &chars, idx);
        }
    }

    pub fn escape_line(&mut self) {
        // Esc deletes the logical line itself, so the rows below move up.
        // An emptied line from Backspace still occupies its row; this does not.
        let (start, end, _, _) = self.content_and_cursor();
        let editable = self.editable_rows();
        let mut kept: Vec<Row> = self.rows[..start].to_vec();
        if end + 1 < editable {
            kept.extend(self.rows[end + 1..editable].iter().cloned());
        }
        while kept.len() < editable {
            kept.push(self.blank_row());
        }
        kept.truncate(editable);
        for (index, row) in kept.into_iter().enumerate() {
            self.rows[index] = row;
        }
        self.cursor_row = start.min(editable.saturating_sub(1));
        self.cursor_col = 0;
    }

    pub fn erase_to_end(&mut self) {
        let (start, end, mut chars, idx) = self.content_and_cursor();
        if idx < chars.len() {
            chars.truncate(idx);
        }
        self.splice_logical(start, end, &chars, idx.min(chars.len()));
    }

    pub fn end_of_line(&mut self) {
        let (start, _) = self.span_at(self.cursor_row);
        let chars = self.read_stripped(start, self.span_at(self.cursor_row).1);
        let idx = chars.len();
        let row = start + idx / self.width;
        let col = idx % self.width;
        let limit = self.editable_rows() - 1;
        if row > limit {
            self.cursor_row = limit;
            self.cursor_col = self.width - 1;
        } else {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    pub fn tab(&mut self) {
        let next = (self.cursor_col / 8 + 1) * 8;
        if self.insert {
            let count = if next >= self.width {
                (self.width - self.cursor_col).max(1)
            } else {
                next - self.cursor_col
            };
            for _ in 0..count {
                let before = self.cursor();
                self.type_char(b' ');
                if self.cursor() == before {
                    break;
                }
            }
        } else if next < self.width {
            self.cursor_col = next;
        } else if self.cursor_row + 1 < self.editable_rows() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn ctrl_enter(&mut self) {
        let editable = self.editable_rows();
        if self.cursor_row + 1 >= editable {
            return;
        }
        let (start, end) = self.span_at(self.cursor_row);
        if self.cursor_row == end {
            let mut next_end = end + 1;
            while next_end + 1 < editable && self.rows[next_end].continues {
                next_end += 1;
            }
            let rest = self.read_stripped(end + 1, next_end);
            let approx = (self.cursor_row - start + 1) * self.width + rest.len();
            if approx > MAX_LOGICAL {
                return;
            }
        }
        self.rows[self.cursor_row].continues = true;
        self.cursor_row += 1;
        self.cursor_col = 0;
    }

    /// Submit the logical line under the cursor and park the cursor where
    /// the command's output should begin.
    pub fn take_logical_line(&mut self) -> String {
        let (start, end) = self.span_at(self.cursor_row);
        let text = bytes_to_string(&self.read_stripped(start, end));
        let blank_extension =
            end > start && self.row_is_blank(end) && self.cursor_row == end && self.cursor_col == 0;
        if blank_extension {
            // The extra row exists only because the cursor wrapped off a full row.
            self.rows[end - 1].continues = false;
        } else {
            let next = end + 1;
            let editable = self.editable_rows();
            if next >= editable {
                self.scroll_up();
                self.cursor_row = editable - 1;
                self.cursor_col = 0;
            } else {
                self.cursor_row = next;
                self.cursor_col = 0;
            }
        }
        text
    }

    pub fn break_line(&mut self) {
        let editable = self.editable_rows();
        if self.cursor_row + 1 >= editable {
            self.scroll_up();
            self.cursor_row = editable - 1;
        } else {
            self.cursor_row += 1;
        }
        self.cursor_col = 0;
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.width - 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_col + 1 < self.width {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.editable_rows() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.editable_rows() {
            self.cursor_row += 1;
        }
    }

    pub fn word_left(&mut self) {
        let mut i = self.flat_index();
        if i == 0 {
            return;
        }
        i -= 1;
        while i > 0 && !is_word(self.cell_at_flat(i)) {
            i -= 1;
        }
        while i > 0 && is_word(self.cell_at_flat(i - 1)) {
            i -= 1;
        }
        self.set_flat(i);
    }

    pub fn word_right(&mut self) {
        let max = self.editable_rows() * self.width;
        let mut i = self.flat_index();
        if i < max && is_word(self.cell_at_flat(i)) {
            while i < max && is_word(self.cell_at_flat(i)) {
                i += 1;
            }
        }
        while i < max && !is_word(self.cell_at_flat(i)) {
            i += 1;
        }
        if i >= max {
            i = max - 1;
        }
        self.set_flat(i);
    }

    fn blank_row(&self) -> Row {
        Row {
            cells: vec![
                Cell {
                    ch: b' ',
                    attr: self.attr
                };
                self.width
            ],
            continues: false,
        }
    }

    fn clamp_cursor(&mut self) {
        let editable = self.editable_rows().max(1);
        if self.cursor_row >= editable {
            self.cursor_row = editable - 1;
        }
        if self.cursor_col >= self.width {
            self.cursor_col = self.width - 1;
        }
    }

    fn span_at(&self, row: usize) -> (usize, usize) {
        let editable = self.editable_rows();
        let row = row.min(editable - 1);
        let mut start = row;
        while start > 0 && self.rows[start - 1].continues {
            start -= 1;
        }
        let mut end = row;
        while end + 1 < editable && self.rows[end].continues {
            end += 1;
        }
        (start, end)
    }

    fn read_stripped(&self, start: usize, end: usize) -> Vec<u8> {
        let mut chars = Vec::new();
        for r in start..=end {
            if r != end {
                chars.extend(self.rows[r].cells.iter().map(|c| c.ch));
            } else {
                let cells = &self.rows[r].cells;
                let mut n = cells.len();
                while n > 0 && cells[n - 1].ch == b' ' {
                    n -= 1;
                }
                chars.extend(cells[..n].iter().map(|c| c.ch));
            }
        }
        if chars.len() > MAX_LOGICAL {
            chars.truncate(MAX_LOGICAL);
        }
        while chars.last() == Some(&b' ') {
            chars.pop();
        }
        chars
    }

    fn content_and_cursor(&self) -> (usize, usize, Vec<u8>, usize) {
        let (start, end) = self.span_at(self.cursor_row);
        let mut chars = Vec::new();
        for r in start..=end {
            if r != end {
                chars.extend(self.rows[r].cells.iter().map(|c| c.ch));
            } else {
                let cells = &self.rows[r].cells;
                let mut n = cells.len();
                while n > 0 && cells[n - 1].ch == b' ' {
                    n -= 1;
                }
                if r == self.cursor_row {
                    n = n.max(self.cursor_col.min(cells.len()));
                }
                chars.extend(cells[..n].iter().map(|c| c.ch));
            }
        }
        if chars.len() > MAX_LOGICAL {
            chars.truncate(MAX_LOGICAL);
        }
        let mut idx = (self.cursor_row - start) * self.width + self.cursor_col;
        if idx > MAX_LOGICAL {
            idx = MAX_LOGICAL;
        }
        if idx > chars.len() {
            let room = MAX_LOGICAL - chars.len();
            let need = (idx - chars.len()).min(room);
            chars.resize(chars.len() + need, b' ');
            idx = idx.min(chars.len());
        }
        (start, end, chars, idx)
    }

    fn splice_logical(&mut self, start: usize, old_end: usize, chars: &[u8], cursor_idx: usize) {
        let editable = self.editable_rows();
        let chars = if chars.len() > MAX_LOGICAL {
            &chars[..MAX_LOGICAL]
        } else {
            chars
        };
        let cursor_idx = cursor_idx.min(chars.len());
        let mut occupied = if chars.is_empty() {
            1
        } else {
            chars.len().div_ceil(self.width)
        };
        if !chars.is_empty() && cursor_idx == chars.len() && chars.len() % self.width == 0 {
            occupied += 1;
        }

        let mut logical_rows = Vec::with_capacity(occupied);
        for i in 0..occupied {
            let mut cells = vec![
                Cell {
                    ch: b' ',
                    attr: self.attr
                };
                self.width
            ];
            for (c, cell) in cells.iter_mut().enumerate() {
                let n = i * self.width + c;
                if n < chars.len() {
                    cell.ch = chars[n];
                }
            }
            logical_rows.push(Row {
                cells,
                continues: i + 1 < occupied,
            });
        }

        let mut before: Vec<Row> = self.rows[..start].to_vec();
        let mut after: Vec<Row> = self.rows[old_end + 1..editable].to_vec();
        while before.len() + logical_rows.len() + after.len() > editable {
            if !after.is_empty() {
                after.pop();
            } else if !before.is_empty() {
                before.remove(0);
            } else {
                break;
            }
        }

        let new_start = before.len();
        let mut assembled = before;
        assembled.extend(logical_rows);
        assembled.extend(after);
        while assembled.len() < editable {
            assembled.push(self.blank_row());
        }
        assembled.truncate(editable);
        for (i, row) in assembled.into_iter().enumerate() {
            self.rows[i] = row;
        }

        let (row_off, col) = if chars.is_empty() {
            (0, 0)
        } else if cursor_idx == chars.len() && chars.len() % self.width == 0 {
            (chars.len() / self.width, 0)
        } else {
            (cursor_idx / self.width, cursor_idx % self.width)
        };
        self.cursor_row = new_start + row_off;
        self.cursor_col = col;
        self.clamp_cursor();
    }

    fn newline(&mut self) {
        if self.cursor_col == 0
            && self.cursor_row > 0
            && self.rows[self.cursor_row - 1].continues
            && self.row_is_blank(self.cursor_row)
        {
            // A print that exactly filled the row already wrapped. The CR lands here.
            self.rows[self.cursor_row - 1].continues = false;
            return;
        }
        let editable = self.editable_rows();
        if self.cursor_row + 1 >= editable {
            self.scroll_up();
            self.cursor_row = editable - 1;
        } else {
            self.cursor_row += 1;
        }
        self.cursor_col = 0;
    }

    fn put_print_char(&mut self, ch: u8) {
        let editable = self.editable_rows();
        if self.cursor_row >= editable {
            self.scroll_up();
            self.cursor_row = editable - 1;
        }
        self.rows[self.cursor_row].cells[self.cursor_col] = Cell {
            ch,
            attr: self.attr,
        };
        self.cursor_col += 1;
        if self.cursor_col >= self.width {
            self.rows[self.cursor_row].continues = true;
            self.cursor_col = 0;
            if self.cursor_row + 1 >= editable {
                self.scroll_up();
                self.cursor_row = editable - 1;
            } else {
                self.cursor_row += 1;
            }
        }
    }

    fn scroll_up(&mut self) {
        let editable = self.editable_rows();
        if editable == 0 {
            return;
        }
        for r in 0..editable - 1 {
            self.rows[r] = self.rows[r + 1].clone();
        }
        self.rows[editable - 1] = self.blank_row();
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
    }

    fn row_is_blank(&self, row: usize) -> bool {
        self.rows[row].cells.iter().all(|c| c.ch == b' ')
    }

    fn flat_index(&self) -> usize {
        self.cursor_row * self.width + self.cursor_col
    }

    fn set_flat(&mut self, idx: usize) {
        let max = self.editable_rows() * self.width;
        let idx = idx.min(max.saturating_sub(1));
        self.cursor_row = idx / self.width;
        self.cursor_col = idx % self.width;
    }

    fn cell_at_flat(&self, idx: usize) -> u8 {
        let row = idx / self.width;
        let col = idx % self.width;
        self.rows[row].cells[col].ch
    }

    fn set_graphics_width(&mut self, width: usize) -> bool {
        let next = match (self.gfx.mode, width) {
            (1, 80) => 2,
            (2, 40) => 1,
            (7, 80) => 8,
            (8, 40) => 7,
            (9, 40) => 7,
            (1, 40)
            | (2, 80)
            | (7, 40)
            | (8, 80)
            | (9, 80)
            | (10, 80)
            | (11, 80)
            | (12, 80)
            | (13, 40) => return true,
            _ => return false,
        };
        self.enter(next, self.gfx.colorburst, true);
        self.insert = false;
        true
    }
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorKind {
    Text,
    /// `SCREEN 1`. Attribute 0 can be a 64-colour value; the other three follow the CGA palette.
    Cga,
    /// `SCREEN 2` and `SCREEN 11`. `COLOR` is illegal.
    Mono2,
    /// `SCREEN 7` and `SCREEN 8`. `COLOR`'s background is taken modulo 8.
    Ega16Mod,
    /// `SCREEN 12`. Sixteen colours, no modulo.
    Ega16,
    /// `SCREEN 9`.
    Ega64,
    /// `SCREEN 10`. Foreground is 1..=3.
    Mono4,
    /// `SCREEN 13`.
    Vga,
}

#[derive(Clone, Copy)]
struct ModeSpec {
    w: usize,
    h: usize,
    cols: usize,
    colors: usize,
    fg: u8,
    kind: ColorKind,
}

fn mode_spec(mode: u8) -> Option<ModeSpec> {
    Some(match mode {
        1 => ModeSpec {
            w: 320,
            h: 200,
            cols: 40,
            colors: 16,
            fg: 3,
            kind: ColorKind::Cga,
        },
        2 => ModeSpec {
            w: 640,
            h: 200,
            cols: 80,
            colors: 16,
            fg: 1,
            kind: ColorKind::Mono2,
        },
        7 => ModeSpec {
            w: 320,
            h: 200,
            cols: 40,
            colors: 16,
            fg: 15,
            kind: ColorKind::Ega16Mod,
        },
        8 => ModeSpec {
            w: 640,
            h: 200,
            cols: 80,
            colors: 16,
            fg: 15,
            kind: ColorKind::Ega16Mod,
        },
        9 => ModeSpec {
            w: 640,
            h: 350,
            cols: 80,
            colors: 64,
            fg: 15,
            kind: ColorKind::Ega64,
        },
        10 => ModeSpec {
            w: 640,
            h: 350,
            cols: 80,
            colors: 16,
            fg: 3,
            kind: ColorKind::Mono4,
        },
        11 => ModeSpec {
            w: 640,
            h: 480,
            cols: 80,
            colors: 16,
            fg: 1,
            kind: ColorKind::Mono2,
        },
        12 => ModeSpec {
            w: 640,
            h: 480,
            cols: 80,
            colors: 16,
            fg: 15,
            kind: ColorKind::Ega16,
        },
        13 => ModeSpec {
            w: 320,
            h: 200,
            cols: 40,
            colors: 256,
            fg: 15,
            kind: ColorKind::Vga,
        },
        _ => return None,
    })
}

/// IBM's 6-bit EGA register: primary bits are 0xAA and secondary bits are 0x55.
/// Bit 0 blue, 1 green, 2 red, 3 secondary blue, 4 secondary green, 5 secondary red.
fn ega64(index: u16) -> u32 {
    let i = index & 63;
    let level = |n: u16| match n {
        0 => 0u32,
        1 => 0x55,
        2 => 0xAA,
        _ => 0xFF,
    };
    let red = level(2 * ((i >> 2) & 1) + ((i >> 5) & 1));
    let green = level(2 * ((i >> 1) & 1) + ((i >> 4) & 1));
    let blue = level(2 * (i & 1) + ((i >> 3) & 1));
    (red << 16) | (green << 8) | blue
}

fn vga256(index: usize) -> u32 {
    if index < 16 {
        return EGA16[index];
    }
    if index < 232 {
        let cube = index - 16;
        let red = (cube / 36) as u32;
        let green = ((cube / 6) % 6) as u32;
        let blue = (cube % 6) as u32;
        return ((red * 51) << 16) | ((green * 51) << 8) | (blue * 51);
    }
    let grey = ((index - 232) as u32 * 255) / 23;
    (grey << 16) | (grey << 8) | grey
}

fn cga_palette(standard: bool) -> Vec<u32> {
    if standard {
        vec![0x000000, 0x00AAAA, 0xAA00AA, 0xAAAAAA]
    } else {
        vec![0x000000, 0x00AA00, 0xAA0000, 0xAA5500]
    }
}

struct WindowMap {
    /// `WINDOW SCREEN` maps `(x0, y0)` to the top left.
    screen: bool,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

struct Gfx {
    mode: u8,
    kind: ColorKind,
    pix_w: usize,
    pix_h: usize,
    pixels: Vec<u8>,
    palette: Vec<u32>,
    n_colors: usize,
    fg: u8,
    colorburst: u8,
    view_l: i32,
    view_t: i32,
    view_r: i32,
    view_b: i32,
    /// `VIEW` without `SCREEN` puts `(0, 0)` at the top left of the viewport.
    view_relative: bool,
    window: Option<WindowMap>,
    pen_x: f64,
    pen_y: f64,
}

impl Gfx {
    fn text() -> Self {
        Self {
            mode: 0,
            kind: ColorKind::Text,
            pix_w: 0,
            pix_h: 0,
            pixels: Vec::new(),
            palette: Vec::new(),
            n_colors: 0,
            fg: 7,
            colorburst: 0,
            view_l: 0,
            view_t: 0,
            view_r: 0,
            view_b: 0,
            view_relative: false,
            window: None,
            pen_x: 0.0,
            pen_y: 0.0,
        }
    }

    fn reset_palette(&mut self) {
        self.palette = match self.kind {
            ColorKind::Text => Vec::new(),
            ColorKind::Cga => cga_palette(self.colorburst == 0),
            ColorKind::Mono2 => vec![EGA16[0], EGA16[15]],
            ColorKind::Ega16 | ColorKind::Ega16Mod => EGA16.to_vec(),
            ColorKind::Ega64 => EGA64_DEFAULT.iter().copied().map(ega64).collect(),
            ColorKind::Mono4 => vec![0x000000, 0xAAAAAA, 0xFFFFFF, 0x555555],
            ColorKind::Vga => (0..256).map(vga256).collect(),
        };
    }

    fn colour_rgb(&self, index: u16) -> u32 {
        match self.kind {
            ColorKind::Ega64 => ega64(index),
            ColorKind::Vga => vga256(usize::from(index)),
            _ => EGA16[usize::from(index & 15)],
        }
    }

    fn span(&self) -> (f64, f64) {
        let w = f64::from(self.view_r - self.view_l + 1);
        let h = f64::from(self.view_b - self.view_t + 1);
        (w, h)
    }

    /// Local coordinates run from 0 at the viewport's top left to `size - 1`.
    fn logical_to_local(&self, lx: f64, ly: f64) -> (f64, f64) {
        let (vw, vh) = self.span();
        let Some(window) = &self.window else {
            return if self.view_relative {
                (lx, ly)
            } else {
                (lx - f64::from(self.view_l), ly - f64::from(self.view_t))
            };
        };
        let x = if vw <= 1.0 {
            0.0
        } else {
            (lx - window.x0) / (window.x1 - window.x0) * (vw - 1.0)
        };
        let y = if vh <= 1.0 {
            0.0
        } else if window.screen {
            (ly - window.y0) / (window.y1 - window.y0) * (vh - 1.0)
        } else {
            (ly - window.y1) / (window.y0 - window.y1) * (vh - 1.0)
        };
        (x, y)
    }

    fn local_to_logical(&self, lx: f64, ly: f64) -> (f64, f64) {
        let (vw, vh) = self.span();
        let Some(window) = &self.window else {
            return if self.view_relative {
                (lx, ly)
            } else {
                (f64::from(self.view_l) + lx, f64::from(self.view_t) + ly)
            };
        };
        let x = if vw <= 1.0 {
            window.x0
        } else {
            window.x0 + lx / (vw - 1.0) * (window.x1 - window.x0)
        };
        let y = if vh <= 1.0 {
            window.y0
        } else if window.screen {
            window.y0 + ly / (vh - 1.0) * (window.y1 - window.y0)
        } else {
            window.y1 + ly / (vh - 1.0) * (window.y0 - window.y1)
        };
        (x, y)
    }

    fn logical_to_viewport(&self, lx: f64, ly: f64) -> (f64, f64) {
        let (local_x, local_y) = self.logical_to_local(lx, ly);
        if self.view_relative {
            (local_x, local_y)
        } else {
            (
                f64::from(self.view_l) + local_x,
                f64::from(self.view_t) + local_y,
            )
        }
    }

    fn viewport_to_logical(&self, vx: f64, vy: f64) -> (f64, f64) {
        let local_x = if self.view_relative {
            vx
        } else {
            vx - f64::from(self.view_l)
        };
        let local_y = if self.view_relative {
            vy
        } else {
            vy - f64::from(self.view_t)
        };
        self.local_to_logical(local_x, local_y)
    }

    fn to_physical(&self, lx: f64, ly: f64) -> Result<(i32, i32), GfxErr> {
        let (local_x, local_y) = self.logical_to_local(lx, ly);
        let px = f64::from(self.view_l) + local_x;
        let py = f64::from(self.view_t) + local_y;
        Ok((round_px(px)?, round_px(py)?))
    }

    fn reset_pen(&mut self) {
        let (vw, vh) = self.span();
        let (vx, vy) = if self.view_relative {
            (vw / 2.0, vh / 2.0)
        } else {
            (
                f64::from(self.view_l) + vw / 2.0,
                f64::from(self.view_t) + vh / 2.0,
            )
        };
        let (lx, ly) = self.viewport_to_logical(vx, vy);
        self.pen_x = lx;
        self.pen_y = ly;
    }

    fn in_view(&self, x: i32, y: i32) -> bool {
        x >= self.view_l && x <= self.view_r && y >= self.view_t && y <= self.view_b
    }

    fn on_screen(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.pix_w && (y as usize) < self.pix_h
    }

    fn put(&mut self, x: i32, y: i32, attr: u8) {
        if !self.on_screen(x, y) {
            return;
        }
        let index = (y as usize) * self.pix_w + (x as usize);
        self.pixels[index] = attr;
    }

    fn clamp_attr(&self, attrib: u8) -> u8 {
        let last = self.palette.len().saturating_sub(1);
        if usize::from(attrib) > last {
            last as u8
        } else {
            attrib
        }
    }

    fn map_axis(&self, coord: f64, vertical: bool, to_logical: bool) -> Result<f64, GfxErr> {
        if to_logical && !in_i16(coord) {
            return Err(GfxErr::Overflow);
        }
        let (x, y) = if to_logical {
            if vertical {
                self.viewport_to_logical(0.0, coord)
            } else {
                self.viewport_to_logical(coord, 0.0)
            }
        } else if vertical {
            self.logical_to_viewport(0.0, coord)
        } else {
            self.logical_to_viewport(coord, 0.0)
        };
        let result = if vertical { y } else { x };
        if !to_logical && !in_i16(result) {
            return Err(GfxErr::Overflow);
        }
        if !result.is_finite() {
            return Err(GfxErr::Overflow);
        }
        Ok(result)
    }

    fn locate(&self, x: f64, y: f64) -> Result<(i32, i32), GfxErr> {
        if !in_i16(x) || !in_i16(y) {
            return Err(GfxErr::Overflow);
        }
        self.to_physical(x, y)
    }

    fn pixel_f(&self, lx: f64, ly: f64) -> (f64, f64) {
        let (local_x, local_y) = self.logical_to_local(lx, ly);
        (
            f64::from(self.view_l) + local_x,
            f64::from(self.view_t) + local_y,
        )
    }

    fn logical_from_pixel(&self, px: f64, py: f64) -> (f64, f64) {
        let vx = if self.view_relative {
            px - f64::from(self.view_l)
        } else {
            px
        };
        let vy = if self.view_relative {
            py - f64::from(self.view_t)
        } else {
            py
        };
        self.viewport_to_logical(vx, vy)
    }

    fn default_aspect(&self) -> f64 {
        if self.pix_w == 0 {
            return 1.0;
        }
        4.0 * self.pix_h as f64 / (3.0 * self.pix_w as f64)
    }

    fn axis_scale(&self) -> (f64, f64) {
        let (vw, vh) = self.span();
        let Some(window) = &self.window else {
            return (1.0, 1.0);
        };
        let xs = (vw - 1.0) / (window.x1 - window.x0);
        let ys = if window.screen {
            (vh - 1.0) / (window.y1 - window.y0)
        } else {
            (vh - 1.0) / (window.y0 - window.y1)
        };
        (xs.abs(), ys.abs())
    }

    fn segment(&mut self, from: (i32, i32), to: (i32, i32), attr: u8, style: Option<u16>) {
        let mut mask = style.unwrap_or(0xFFFF);
        let styled = style.is_some();
        walk_line(from.0, from.1, to.0, to.1, |x, y| {
            let plot = !styled || mask & 0x8000 != 0;
            if styled {
                mask = mask.rotate_left(1);
            }
            if plot && self.in_view(x, y) {
                self.put(x, y, attr);
            }
        });
    }

    fn box_outline(&mut self, a: (i32, i32), b: (i32, i32), attr: u8, style: Option<u16>) {
        let (x0, y0) = a;
        let (x1, y1) = b;
        if x0 == x1 || y0 == y1 {
            self.segment(a, b, attr, style);
            return;
        }
        self.segment((x0, y0), (x1, y0), attr, style);
        self.segment((x1, y0), (x1, y1), attr, style);
        self.segment((x1, y1), (x0, y1), attr, style);
        self.segment((x0, y1), (x0, y0), attr, style);
    }

    fn fill_rect(&mut self, a: (i32, i32), b: (i32, i32), attr: u8) {
        let left = a.0.min(b.0).max(self.view_l);
        let right = a.0.max(b.0).min(self.view_r);
        let top = a.1.min(b.1).max(self.view_t);
        let bottom = a.1.max(b.1).min(self.view_b);
        if left > right || top > bottom {
            return;
        }
        for y in top..=bottom {
            for x in left..=right {
                self.put(x, y, attr);
            }
        }
    }

    fn ellipse(&mut self, center: (i32, i32), radii: (i32, i32), arc: (f64, f64), attr: u8) {
        let (cx, cy) = center;
        let (rx, ry) = radii;
        let (start, span) = arc;
        let mut plot = |x: i32, y: i32| {
            if on_arc(x - cx, y - cy, start, span) && self.in_view(x, y) {
                self.put(x, y, attr);
            }
        };
        if rx == 0 && ry == 0 {
            if span >= std::f64::consts::TAU - 1e-6 {
                plot(cx, cy);
            }
            return;
        }
        if rx == 0 {
            for y in -ry..=ry {
                plot(cx, cy + y);
            }
            return;
        }
        if ry == 0 {
            for x in -rx..=rx {
                plot(cx + x, cy);
            }
            return;
        }
        let rx = i64::from(rx);
        let ry = i64::from(ry);
        let rx2 = rx * rx;
        let ry2 = ry * ry;
        let tworx2 = 2 * rx2;
        let twory2 = 2 * ry2;
        let mut x = 0i64;
        let mut y = ry;
        let mut px = 0i64;
        let mut py = tworx2 * y;
        let mut p = ry2 - rx2 * ry + rx2 / 4;
        while px < py {
            plot_quad(cx, cy, x, y, &mut plot);
            x += 1;
            px += twory2;
            if p < 0 {
                p += ry2 + px;
            } else {
                y -= 1;
                py -= tworx2;
                p += ry2 + px - py;
            }
        }
        p = ry2 * (x * x + x) + ry2 / 4 + rx2 * (y - 1) * (y - 1) - rx2 * ry2;
        while y >= 0 {
            plot_quad(cx, cy, x, y, &mut plot);
            y -= 1;
            py -= tworx2;
            if p > 0 {
                p += rx2 - py;
            } else {
                x += 1;
                px += twory2;
                p += rx2 - py + px;
            }
        }
    }

    fn spoke(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, angle: f64, attr: u8) {
        let ex = f64::from(cx) + f64::from(rx) * angle.cos();
        let ey = f64::from(cy) - f64::from(ry) * angle.sin();
        let Ok(ex) = round_px(ex) else {
            return;
        };
        let Ok(ey) = round_px(ey) else {
            return;
        };
        self.segment((cx, cy), (ex, ey), attr, None);
    }

    fn flood(
        &mut self,
        x: f64,
        y: f64,
        ink: FloodInk,
        border: u8,
        background: Option<Vec<u8>>,
    ) -> Result<(), GfxErr> {
        let (sx, sy) = self.locate(x, y)?;
        if !self.in_view(sx, sy) {
            return Ok(());
        }
        let border = self.clamp_attr(border);
        if self.pixel_attr(sx, sy) == border {
            return Ok(());
        }
        let mut seen = vec![false; self.pix_w.saturating_mul(self.pix_h)];
        let mut stack = vec![(sx, sy)];
        while let Some((mut x, y)) = stack.pop() {
            while self.paintable(x - 1, y, border, &background, &seen) {
                x -= 1;
            }
            let mut above = false;
            let mut below = false;
            while self.paintable(x, y, border, &background, &seen) {
                let color = match &ink {
                    FloodInk::Solid(color) => *color,
                    FloodInk::Tile(tile) => self.tile_attr(tile, x, y),
                };
                self.put(x, y, color);
                let index = self.pix_index(x, y);
                if index < seen.len() {
                    seen[index] = true;
                }
                above = self.queue_span((x, y - 1), border, &background, &seen, above, &mut stack);
                below = self.queue_span((x, y + 1), border, &background, &seen, below, &mut stack);
                x += 1;
            }
        }
        Ok(())
    }

    fn paintable(
        &self,
        x: i32,
        y: i32,
        border: u8,
        background: &Option<Vec<u8>>,
        seen: &[bool],
    ) -> bool {
        if !self.in_view(x, y) {
            return false;
        }
        let index = self.pix_index(x, y);
        if index >= seen.len() || seen[index] {
            return false;
        }
        let attr = self.pixel_attr(x, y);
        if attr == border {
            return false;
        }
        if let Some(tile) = background {
            return attr == self.tile_attr(tile, x, y);
        }
        true
    }

    fn queue_span(
        &self,
        at: (i32, i32),
        border: u8,
        background: &Option<Vec<u8>>,
        seen: &[bool],
        in_span: bool,
        stack: &mut Vec<(i32, i32)>,
    ) -> bool {
        let (x, y) = at;
        if self.paintable(x, y, border, background, seen) {
            if !in_span {
                stack.push((x, y));
            }
            true
        } else {
            false
        }
    }

    fn pix_index(&self, x: i32, y: i32) -> usize {
        (y as usize) * self.pix_w + (x as usize)
    }

    fn pixel_attr(&self, x: i32, y: i32) -> u8 {
        self.pixels[self.pix_index(x, y)]
    }

    fn tile_attr(&self, tile: &[u8], x: i32, y: i32) -> u8 {
        if tile.is_empty() {
            return 0;
        }
        let row = tile[(y as usize) % tile.len()];
        let bpp = match self.palette.len() {
            0..=2 => 1,
            3..=4 => 2,
            5..=16 => 4,
            _ => 8,
        };
        let pixels_per_byte = 8 / bpp;
        let slot = (x as usize) % pixels_per_byte;
        let shift = 8 - bpp - slot * bpp;
        let mask = (1u16 << bpp) - 1;
        let attr = u16::from(row) >> shift & mask;
        self.clamp_attr(attr as u8)
    }
}

enum FloodInk {
    Solid(u8),
    Tile(Vec<u8>),
}

fn check_tile(bytes: &[u8]) -> Result<(), GfxErr> {
    if bytes.is_empty() || bytes.len() > 64 {
        Err(GfxErr::Illegal)
    } else {
        Ok(())
    }
}

fn norm_angle(angle: f64) -> Result<(f64, bool), GfxErr> {
    if !angle.is_finite() {
        return Err(GfxErr::Overflow);
    }
    let spoke = angle < 0.0;
    let magnitude = angle.abs();
    let tau = std::f64::consts::TAU;
    if magnitude > tau + 1.0e-3 {
        return Err(GfxErr::Illegal);
    }
    Ok((magnitude.min(tau), spoke))
}

fn round_len(v: f64) -> Result<i32, GfxErr> {
    if !v.is_finite() || v < 0.0 {
        return Err(GfxErr::Overflow);
    }
    let rounded = (v + 0.5).floor();
    if rounded > 32767.0 {
        return Err(GfxErr::Overflow);
    }
    Ok(rounded as i32)
}

fn on_arc(dx: i32, dy: i32, start: f64, span: f64) -> bool {
    let tau = std::f64::consts::TAU;
    if span >= tau - 1.0e-6 {
        return true;
    }
    if span <= 1.0e-9 {
        return false;
    }
    let mut angle = (-f64::from(dy)).atan2(f64::from(dx));
    if angle < 0.0 {
        angle += tau;
    }
    let mut delta = angle - start;
    if delta < 0.0 {
        delta += tau;
    }
    delta <= span + 1.0e-6
}

fn plot_quad(cx: i32, cy: i32, x: i64, y: i64, plot: &mut impl FnMut(i32, i32)) {
    let x = x as i32;
    let y = y as i32;
    plot(cx + x, cy + y);
    plot(cx - x, cy + y);
    plot(cx + x, cy - y);
    plot(cx - x, cy - y);
}

fn walk_line(x0: i32, y0: i32, x1: i32, y1: i32, mut visit: impl FnMut(i32, i32)) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;
    loop {
        visit(x, y);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = err * 2;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

fn in_i16(v: f64) -> bool {
    v.is_finite() && (-32768.0..=32767.0).contains(&v)
}

fn round_px(v: f64) -> Result<i32, GfxErr> {
    if !v.is_finite() {
        return Err(GfxErr::Overflow);
    }
    let rounded = if v >= 0.0 {
        (v + 0.5).floor()
    } else {
        (v - 0.5).ceil()
    };
    if !(-32768.0..=32767.0).contains(&rounded) {
        return Err(GfxErr::Overflow);
    }
    Ok(rounded as i32)
}

fn byte_arg(n: i16) -> Result<u8, GfxErr> {
    if (0..=255).contains(&n) {
        Ok(n as u8)
    } else {
        Err(GfxErr::Illegal)
    }
}

impl Screen {
    pub fn graphics_mode(&self) -> u8 {
        self.gfx.mode
    }

    pub fn pixel_size(&self) -> (usize, usize) {
        (self.gfx.pix_w, self.gfx.pix_h)
    }

    pub fn pixel_rgb(&self, x: usize, y: usize) -> u32 {
        if x >= self.gfx.pix_w || y >= self.gfx.pix_h {
            return 0;
        }
        let attr = self.gfx.pixels[y * self.gfx.pix_w + x];
        self.attribute_rgb(attr)
    }

    pub fn attribute_rgb(&self, attr: u8) -> u32 {
        self.gfx
            .palette
            .get(usize::from(attr))
            .copied()
            .unwrap_or(EGA16[usize::from(attr & 0x0F)])
    }

    pub fn apply_screen(&mut self, args: ScreenArgs) -> Result<(), GfxErr> {
        if args.erase {
            return Err(GfxErr::Illegal);
        }
        let mode = if let Some(mode) = args.mode {
            if !(0..=255).contains(&mode) {
                return Err(GfxErr::Illegal);
            }
            let mode = mode as u8;
            if mode != 0 && mode_spec(mode).is_none() {
                return Err(GfxErr::Illegal);
            }
            mode
        } else {
            self.gfx.mode
        };
        let burst = if let Some(burst) = args.burst {
            byte_arg(burst)?
        } else {
            self.gfx.colorburst
        };
        for page in [args.active, args.visible].into_iter().flatten() {
            if page != 0 {
                return Err(GfxErr::Illegal);
            }
        }
        let changed = mode != self.gfx.mode;
        let burst_flip = (burst == 0) != (self.gfx.colorburst == 0);
        let clear = mode == 0 || changed || burst_flip;
        self.enter(mode, burst, clear);
        Ok(())
    }

    pub fn plot(
        &mut self,
        step: bool,
        x: f64,
        y: f64,
        attrib: Option<u8>,
        preset: bool,
    ) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        if !in_i16(x) || !in_i16(y) {
            return Err(GfxErr::Overflow);
        }
        let (lx, ly) = if step {
            (self.gfx.pen_x + x, self.gfx.pen_y + y)
        } else {
            (x, y)
        };
        let (px, py) = self.gfx.to_physical(lx, ly)?;
        self.gfx.pen_x = lx;
        self.gfx.pen_y = ly;
        let attr = match attrib {
            Some(attr) => self.gfx.clamp_attr(attr),
            None if preset => 0,
            None => self.gfx.fg,
        };
        if self.gfx.in_view(px, py) {
            self.gfx.put(px, py, attr);
        }
        Ok(())
    }

    pub fn point_fn(&self, func: i16) -> Result<f64, GfxErr> {
        if !(0..=3).contains(&func) {
            return Err(GfxErr::Illegal);
        }
        let (vx, vy) = self.gfx.logical_to_viewport(self.gfx.pen_x, self.gfx.pen_y);
        let value = match func {
            0 => vx,
            1 => vy,
            2 => self.gfx.pen_x,
            _ => self.gfx.pen_y,
        };
        if !value.is_finite() {
            return Err(GfxErr::Overflow);
        }
        Ok(value)
    }

    pub fn point_xy(&self, x: f64, y: f64) -> Result<i16, GfxErr> {
        if !in_i16(x) || !in_i16(y) {
            return Err(GfxErr::Overflow);
        }
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        let (px, py) = self.gfx.to_physical(x, y)?;
        if !self.gfx.in_view(px, py) {
            return Ok(-1);
        }
        Ok(i16::from(
            self.gfx.pixels[(py as usize) * self.gfx.pix_w + (px as usize)],
        ))
    }

    pub fn pmap(&self, coord: f64, func: i16) -> Result<f64, GfxErr> {
        if !(0..=3).contains(&func) {
            return Err(GfxErr::Illegal);
        }
        if self.gfx.mode == 0 {
            return Ok(0.0);
        }
        if !coord.is_finite() {
            return Err(GfxErr::Overflow);
        }
        self.gfx.map_axis(coord, func & 1 == 1, func >= 2)
    }

    pub fn reset_view(&mut self) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        self.gfx.view_l = 0;
        self.gfx.view_t = 0;
        self.gfx.view_r = self.gfx.pix_w as i32 - 1;
        self.gfx.view_b = self.gfx.pix_h as i32 - 1;
        self.gfx.view_relative = false;
        self.gfx.reset_pen();
        Ok(())
    }

    pub fn set_view(
        &mut self,
        screen_coords: bool,
        corners: [f64; 4],
        fill: Option<u8>,
        border: Option<u8>,
    ) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        let x0 = round_px(corners[0])?;
        let y0 = round_px(corners[1])?;
        let x1 = round_px(corners[2])?;
        let y1 = round_px(corners[3])?;
        if !self.gfx.on_screen(x0, y0) || !self.gfx.on_screen(x1, y1) {
            return Err(GfxErr::Illegal);
        }
        self.gfx.view_l = x0.min(x1);
        self.gfx.view_r = x0.max(x1);
        self.gfx.view_t = y0.min(y1);
        self.gfx.view_b = y0.max(y1);
        self.gfx.view_relative = !screen_coords;
        if let Some(fill) = fill {
            let attr = self.gfx.clamp_attr(fill);
            let (left, top, right, bottom) = (
                self.gfx.view_l,
                self.gfx.view_t,
                self.gfx.view_r,
                self.gfx.view_b,
            );
            for y in top..=bottom {
                for x in left..=right {
                    self.gfx.put(x, y, attr);
                }
            }
        }
        if let Some(border) = border {
            let attr = self.gfx.clamp_attr(border);
            let (left, top, right, bottom) = (
                self.gfx.view_l,
                self.gfx.view_t,
                self.gfx.view_r,
                self.gfx.view_b,
            );
            for x in (left - 1)..=(right + 1) {
                self.gfx.put(x, top - 1, attr);
                self.gfx.put(x, bottom + 1, attr);
            }
            for y in top..=bottom {
                self.gfx.put(left - 1, y, attr);
                self.gfx.put(right + 1, y, attr);
            }
        }
        self.gfx.reset_pen();
        Ok(())
    }

    pub fn set_window(
        &mut self,
        screen_style: bool,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
            return Err(GfxErr::Overflow);
        }
        if x0 == x1 || y0 == y1 {
            return Err(GfxErr::Illegal);
        }
        self.gfx.window = Some(WindowMap {
            screen: screen_style,
            x0,
            y0,
            x1,
            y1,
        });
        self.gfx.reset_pen();
        Ok(())
    }

    pub fn clear_window(&mut self) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        self.gfx.window = None;
        self.gfx.reset_pen();
        Ok(())
    }

    pub fn set_palette(&mut self, assignment: Option<(i16, i16)>) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            return Err(GfxErr::Illegal);
        }
        let Some((attrib, colour)) = assignment else {
            self.gfx.reset_palette();
            return Ok(());
        };
        if attrib < 0 || attrib as usize >= self.gfx.palette.len() {
            return Err(GfxErr::Illegal);
        }
        if colour < -1 || (colour >= 0 && colour as usize >= self.gfx.n_colors) {
            return Err(GfxErr::Illegal);
        }
        if colour >= 0 {
            self.gfx.palette[attrib as usize] = self.gfx.colour_rgb(colour as u16);
        }
        Ok(())
    }

    pub fn color_graphics(&mut self, args: ColorArgs) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 || self.gfx.kind == ColorKind::Mono2 {
            return Err(GfxErr::Illegal);
        }
        match self.gfx.kind {
            ColorKind::Cga => self.color_cga(args),
            ColorKind::Ega16Mod => self.color_foreground(args, 1, 15, 15, true),
            ColorKind::Ega16 => self.color_foreground(args, 1, 15, 15, false),
            ColorKind::Ega64 => self.color_foreground(args, 1, 15, 15, false),
            ColorKind::Mono4 => self.color_foreground(args, 1, 3, 15, false),
            ColorKind::Vga => self.color_foreground(args, 1, 255, 255, false),
            ColorKind::Text | ColorKind::Mono2 => Err(GfxErr::Illegal),
        }
    }

    pub fn screen_cell(&self, row: i16, col: i16, func: i16) -> Result<i16, GfxErr> {
        if !(1..=25).contains(&row) {
            return Err(GfxErr::Illegal);
        }
        if self.keys_on && row == 25 {
            return Err(GfxErr::Illegal);
        }
        if col < 1 || col > self.width as i16 {
            return Err(GfxErr::Illegal);
        }
        if func != 0 && self.gfx.mode != 0 {
            return Ok(0);
        }
        let cell = self.cell((row as usize) - 1, (col as usize) - 1);
        if func == 0 {
            Ok(i16::from(cell.ch))
        } else {
            Ok(i16::from(cell.attr))
        }
    }

    fn enter(&mut self, mode: u8, burst: u8, clear: bool) {
        if mode == 0 {
            self.gfx.mode = 0;
            self.gfx.kind = ColorKind::Text;
            self.gfx.colorburst = burst;
            self.gfx.pixels.clear();
            self.gfx.palette.clear();
            self.gfx.pix_w = 0;
            self.gfx.pix_h = 0;
            self.gfx.n_colors = 0;
            self.attr = ATTR_NORMAL;
            if clear {
                self.clear_text();
            }
            return;
        }
        let Some(spec) = mode_spec(mode) else {
            return;
        };
        self.gfx.colorburst = burst;
        if !clear {
            return;
        }
        self.gfx.mode = mode;
        self.gfx.kind = spec.kind;
        self.gfx.n_colors = spec.colors;
        self.gfx.fg = spec.fg;
        self.gfx.pix_w = spec.w;
        self.gfx.pix_h = spec.h;
        self.gfx.pixels = vec![0; spec.w * spec.h];
        self.gfx.view_l = 0;
        self.gfx.view_t = 0;
        self.gfx.view_r = spec.w as i32 - 1;
        self.gfx.view_b = spec.h as i32 - 1;
        self.gfx.view_relative = false;
        self.gfx.window = None;
        self.gfx.reset_palette();
        self.gfx.reset_pen();
        self.attr = spec.fg;
        self.width = spec.cols;
        self.rows = (0..SCREEN_ROWS).map(|_| self.blank_row()).collect();
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn color_cga(&mut self, args: ColorArgs) -> Result<(), GfxErr> {
        let background = args.foreground.map(byte_arg).transpose()?;
        let palette = args.palette_0.map(byte_arg).transpose()?;
        let over = args.extra.map(byte_arg).transpose()?;
        if let Some(background) = background {
            self.gfx.palette[0] = ega64(u16::from(background) % 64);
        }
        if let Some(choice) = over.or(palette) {
            let colors = cga_palette(choice % 2 == 1);
            self.gfx.palette[1] = colors[1];
            self.gfx.palette[2] = colors[2];
            self.gfx.palette[3] = colors[3];
        }
        Ok(())
    }

    fn color_foreground(
        &mut self,
        args: ColorArgs,
        fg_min: i16,
        fg_max: i16,
        palette_max: i16,
        modulo8: bool,
    ) -> Result<(), GfxErr> {
        if let Some(fg) = args.foreground {
            if fg < fg_min || fg > fg_max {
                return Err(GfxErr::Illegal);
            }
        }
        if let Some(background) = args.palette_0 {
            if background < 0 || background > palette_max {
                return Err(GfxErr::Illegal);
            }
        }
        if let Some(extra) = args.extra {
            byte_arg(extra)?;
        }
        if let Some(fg) = args.foreground {
            self.gfx.fg = fg as u8;
            self.attr = fg as u8;
        }
        if let Some(background) = args.palette_0 {
            let index = if modulo8 { background % 8 } else { background };
            self.gfx.palette[0] = self.gfx.colour_rgb(index as u16);
        }
        Ok(())
    }

    pub fn drawing_color(&self) -> u8 {
        self.gfx.fg
    }

    pub fn set_drawing_color(&mut self, color: u8) -> Result<(), GfxErr> {
        self.require_graphics()?;
        self.gfx.fg = self.gfx.clamp_attr(color);
        self.attr = self.gfx.fg;
        Ok(())
    }

    pub fn pen(&self) -> (f64, f64) {
        (self.gfx.pen_x, self.gfx.pen_y)
    }

    /// `filled` is `None` for a segment, `Some(false)` for a box outline, and
    /// `Some(true)` for a filled box. `style` bits are used high-bit first.
    pub fn line(
        &mut self,
        ends: [f64; 4],
        attr: u8,
        style: Option<u16>,
        filled: Option<bool>,
    ) -> Result<(), GfxErr> {
        self.require_graphics()?;
        let start = self.gfx.locate(ends[0], ends[1])?;
        let end = self.gfx.locate(ends[2], ends[3])?;
        let attr = self.gfx.clamp_attr(attr);
        if filled == Some(true) {
            self.gfx.fill_rect(start, end, attr);
        } else if filled == Some(false) {
            self.gfx.box_outline(start, end, attr, style);
        } else {
            self.gfx.segment(start, end, attr, style);
        }
        self.gfx.pen_x = ends[2];
        self.gfx.pen_y = ends[3];
        Ok(())
    }

    pub fn circle(&mut self, circle: Circle) -> Result<(), GfxErr> {
        self.require_graphics()?;
        if !circle.radius.is_finite() || circle.radius < 0.0 {
            return Err(if circle.radius.is_finite() {
                GfxErr::Illegal
            } else {
                GfxErr::Overflow
            });
        }
        let (cx, cy) = self.gfx.locate(circle.cx, circle.cy)?;
        let aspect = match circle.aspect {
            None => self.gfx.default_aspect(),
            Some(aspect) if aspect.is_finite() && aspect > 0.0 => aspect,
            Some(aspect) if aspect.is_finite() => return Err(GfxErr::Illegal),
            Some(_) => return Err(GfxErr::Overflow),
        };
        let (start, spoke_start) = match circle.start {
            None => (0.0, false),
            Some(angle) => norm_angle(angle)?,
        };
        let (end, spoke_end) = match circle.end {
            None => (std::f64::consts::TAU, false),
            Some(angle) => norm_angle(angle)?,
        };
        let full = circle.start.is_none() && circle.end.is_none();
        let mut span = end - start;
        if span < 0.0 {
            span += std::f64::consts::TAU;
        }
        if full {
            span = std::f64::consts::TAU;
        }
        let (x_scale, y_scale) = self.gfx.axis_scale();
        let (x_aspect, y_aspect) = if aspect < 1.0 {
            (1.0, aspect)
        } else {
            (1.0 / aspect, 1.0)
        };
        let rx = round_len(circle.radius * x_aspect * x_scale)?;
        let ry = round_len(circle.radius * y_aspect * y_scale)?;
        let attr = self.gfx.clamp_attr(circle.attr);
        self.gfx.ellipse((cx, cy), (rx, ry), (start, span), attr);
        if spoke_start {
            self.gfx.spoke(cx, cy, rx, ry, start, attr);
        }
        if spoke_end {
            self.gfx.spoke(cx, cy, rx, ry, end, attr);
        }
        self.gfx.pen_x = circle.cx;
        self.gfx.pen_y = circle.cy;
        Ok(())
    }

    pub fn paint_at(
        &mut self,
        x: f64,
        y: f64,
        fill: Fill,
        border: Option<u8>,
        background: Option<Vec<u8>>,
    ) -> Result<(), GfxErr> {
        self.require_graphics()?;
        let tile = match &fill {
            Fill::Solid(color) => {
                let color = self.gfx.clamp_attr(*color);
                let border = border.unwrap_or(color);
                self.gfx.flood(x, y, FloodInk::Solid(color), border, None)?;
                return Ok(());
            }
            Fill::Tile(bytes) => bytes.clone(),
        };
        check_tile(&tile)?;
        if let Some(bytes) = &background {
            check_tile(bytes)?;
        }
        let border = border.unwrap_or(self.gfx.fg);
        self.gfx
            .flood(x, y, FloodInk::Tile(tile), border, background)?;
        Ok(())
    }

    /// Move the pen by a pixel delta. `draw` strokes the segment.
    pub fn move_pen(&mut self, dx: f64, dy: f64, draw: bool) -> Result<(), GfxErr> {
        self.require_graphics()?;
        if !dx.is_finite() || !dy.is_finite() {
            return Err(GfxErr::Overflow);
        }
        let (px, py) = self.gfx.pixel_f(self.gfx.pen_x, self.gfx.pen_y);
        let nx = px + dx;
        let ny = py + dy;
        if draw {
            let from = (round_px(px)?, round_px(py)?);
            let to = (round_px(nx)?, round_px(ny)?);
            self.gfx.segment(from, to, self.gfx.fg, None);
        }
        let (lx, ly) = self.gfx.logical_from_pixel(nx, ny);
        if !lx.is_finite() || !ly.is_finite() {
            return Err(GfxErr::Overflow);
        }
        self.gfx.pen_x = lx;
        self.gfx.pen_y = ly;
        Ok(())
    }

    pub fn move_pen_to(&mut self, x: f64, y: f64, draw: bool) -> Result<(), GfxErr> {
        self.require_graphics()?;
        let from = self.gfx.locate(self.gfx.pen_x, self.gfx.pen_y)?;
        let to = self.gfx.locate(x, y)?;
        if draw {
            self.gfx.segment(from, to, self.gfx.fg, None);
        }
        self.gfx.pen_x = x;
        self.gfx.pen_y = y;
        Ok(())
    }

    fn require_graphics(&self) -> Result<(), GfxErr> {
        if self.gfx.mode == 0 {
            Err(GfxErr::Illegal)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(screen: &mut Screen, s: &str) {
        for b in s.bytes() {
            screen.type_char(b);
        }
    }

    #[test]
    fn typing_and_enter_reads_the_whole_line_from_column_zero() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "10 REM Z");
        screen.home();
        assert_eq!(screen.take_logical_line(), "10 REM Z");
    }

    #[test]
    fn insert_shifts_the_rest_of_the_line() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "AB");
        screen.move_left();
        screen.set_insert(true);
        screen.type_char(b'C');
        assert_eq!(screen.logical_text(), "ACB");
    }

    #[test]
    fn backspace_at_column_zero_deletes_under_the_cursor() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "AB");
        screen.home();
        screen.backspace();
        assert_eq!(screen.logical_text(), "B");
    }

    #[test]
    fn delete_pulls_the_line_left() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "ABC");
        screen.set_cursor(0, 1);
        screen.delete_char();
        assert_eq!(screen.logical_text(), "AC");
    }

    #[test]
    fn escape_removes_the_logical_line_and_pulls_the_next_one_up() {
        let mut screen = Screen::new(80);
        screen.print_str("ONE\nTWO\n");
        screen.set_cursor(0, 0);
        screen.escape_line();
        assert!(screen.snapshot().starts_with("TWO"));
    }

    #[test]
    fn ctrl_end_keeps_the_prefix() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "ABCDEF");
        screen.set_cursor(0, 3);
        screen.erase_to_end();
        assert_eq!(screen.logical_text(), "ABC");
    }

    #[test]
    fn a_space_in_the_last_column_stays_in_the_logical_line() {
        let mut screen = Screen::new(80);
        let mut text = "A".repeat(79);
        text.push(' ');
        text.push_str("BC");
        type_str(&mut screen, &text);
        assert_eq!(screen.logical_text(), text);
    }

    #[test]
    fn wrap_is_one_logical_line_and_stops_at_255() {
        let mut screen = Screen::new(80);
        for _ in 0..81 {
            screen.type_char(b'X');
        }
        assert!(screen.continues(0));
        assert_eq!(screen.logical_text().len(), 81);
        for _ in 0..300 {
            screen.type_char(b'Y');
        }
        assert_eq!(screen.logical_text().len(), 255);
    }

    #[test]
    fn ctrl_enter_joins_rows() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "HELLO");
        screen.ctrl_enter();
        type_str(&mut screen, "WORLD");
        let text = screen.logical_text();
        assert!(text.starts_with("HELLO"));
        assert!(text.ends_with("WORLD"));
        assert!(text.len() > "HELLO WORLD".len());
    }

    #[test]
    fn word_right_lands_on_the_next_word() {
        let mut screen = Screen::new(80);
        type_str(&mut screen, "AA BB");
        screen.home();
        screen.word_right();
        assert_eq!(screen.cursor(), (0, 3));
    }

    #[test]
    fn full_row_then_enter_does_not_skip_a_line() {
        let mut screen = Screen::new(80);
        for _ in 0..80 {
            screen.type_char(b'A');
        }
        assert_eq!(screen.cursor(), (1, 0));
        let text = screen.take_logical_line();
        assert_eq!(text.len(), 80);
        assert_eq!(screen.cursor(), (1, 0));
    }

    #[test]
    fn ega64_defaults_match_the_ega16_colours() {
        for (index, rgb) in EGA64_DEFAULT.iter().zip(EGA16) {
            assert_eq!(ega64(*index), rgb);
        }
        assert_eq!(ega64(20), 0x00AA_5500);
        assert_eq!(ega64(63), 0x00FF_FFFF);
    }

    #[test]
    fn window_pmap_flips_the_y_axis() {
        let mut screen = Screen::new(80);
        screen
            .apply_screen(ScreenArgs {
                mode: Some(1),
                burst: Some(0),
                active: None,
                visible: None,
                erase: false,
            })
            .unwrap();
        screen.set_window(false, 0.0, 0.0, 319.0, 199.0).unwrap();
        assert_eq!(screen.pmap(0.0, 1).unwrap(), 199.0);
        assert_eq!(screen.pmap(199.0, 1).unwrap(), 0.0);
        screen.set_window(true, 0.0, 0.0, 319.0, 199.0).unwrap();
        assert_eq!(screen.pmap(0.0, 1).unwrap(), 0.0);
        assert_eq!(screen.pmap(199.0, 1).unwrap(), 199.0);
    }

    #[test]
    fn line_style_and_circle_cardinals() {
        let mut screen = Screen::new(80);
        screen
            .apply_screen(ScreenArgs {
                mode: Some(1),
                burst: Some(0),
                active: None,
                visible: None,
                erase: false,
            })
            .unwrap();
        screen
            .line([0.0, 0.0, 3.0, 0.0], 1, Some(0x8000), None)
            .unwrap();
        assert_eq!(screen.point_xy(0.0, 0.0).unwrap(), 1);
        assert_eq!(screen.point_xy(1.0, 0.0).unwrap(), 0);
        assert_eq!(screen.point_fn(0).unwrap().round(), 3.0);

        screen
            .circle(Circle {
                cx: 160.0,
                cy: 100.0,
                radius: 10.0,
                attr: 2,
                start: None,
                end: None,
                aspect: Some(1.0),
            })
            .unwrap();
        assert_eq!(screen.point_xy(170.0, 100.0).unwrap(), 2);
        assert_eq!(screen.point_xy(150.0, 100.0).unwrap(), 2);
        assert_eq!(screen.point_xy(160.0, 90.0).unwrap(), 2);
        assert_eq!(screen.point_xy(160.0, 110.0).unwrap(), 2);
        assert_eq!(screen.point_xy(160.0, 100.0).unwrap(), 0);
        assert_eq!(screen.pen(), (160.0, 100.0));

        screen.set_drawing_color(1).unwrap();
        assert_eq!(screen.drawing_color(), 1);
    }
}
