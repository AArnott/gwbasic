//! The GW-BASIC screen. There is no separate line buffer: the cells on the
//! screen are the line. A logical line is one or more physical rows joined by
//! a continuation flag, at most 255 characters.

pub const SCREEN_ROWS: usize = 25;
pub const MAX_LOGICAL: usize = 255;
pub const ATTR_NORMAL: u8 = 0x07;

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
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
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
}
