use std::collections::VecDeque;

use glyphon::Color;
use vte::{Parser, Perform};

const MAX_SCROLLBACK: usize = 200;
const ROWS: usize = 24;
const COLS: usize = 80;

pub mod style {
    pub const BOLD: u8 = 1 << 0;
    pub const ITALIC: u8 = 1 << 1;
    pub const UNDERLINE: u8 = 1 << 2;
    pub const STRIKETHROUGH: u8 = 1 << 3;
    pub const DIM: u8 = 1 << 4;
    pub const BLINK: u8 = 1 << 5;
    pub const REVERSE: u8 = 1 << 6;
    pub const HIDDEN: u8 = 1 << 7;
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub style: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::rgb(255, 255, 255),
            bg: Color::rgb(0, 0, 0),
            style: 0,
        }
    }
}

pub struct Performer {
    pub grid: VecDeque<Vec<Cell>>,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub cols: usize,
    pub rows: usize,
    pub current_fg: Color,
    pub current_bg: Color,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: bool,
    pub cursor_visible: bool,
    pub current_style: u8,
}

#[derive(Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Default)]
pub struct Terminal {
    pub parser: Parser,
    pub performer: Performer,
}

impl Default for Performer {
    fn default() -> Self {
        Self {
            grid: VecDeque::from(vec![vec![Cell::default(); COLS]; ROWS]),
            scrollback: VecDeque::new(),
            cursor_x: 0,
            cursor_y: 0,
            cols: COLS,
            rows: ROWS,
            current_fg: Color::rgb(255, 255, 255),
            current_bg: Color::rgb(0, 0, 0),
            current_style: 0,
            cursor_style: CursorStyle::Block,
            cursor_blinking: false,
            cursor_visible: true,
        }
    }
}

impl Terminal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.performer, bytes);
    }

    /// Returns a slice of the visible grid rows for styled rendering.
    pub fn visible_rows(&self, scroll_offset: i32, rows: usize) -> Vec<&Vec<Cell>> {
        if rows == 0 {
            return vec![];
        }

        let scrollback_len = self.performer.scrollback.len();
        let grid_len = self.performer.grid.len();
        let total_rows = scrollback_len + grid_len;
        if total_rows == 0 {
            return vec![];
        }

        let offset = scroll_offset.max(0) as usize;
        let end = total_rows.saturating_sub(offset);
        let start = end.saturating_sub(rows);

        (start..end)
            .map(|idx| {
                if idx < scrollback_len {
                    &self.performer.scrollback[idx]
                } else {
                    &self.performer.grid[idx - scrollback_len]
                }
            })
            .collect()
    }
}

impl Performer {
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        self.grid.resize(
            new_rows,
            vec![
                Cell {
                    c: ' ',
                    fg: self.current_fg,
                    bg: self.current_bg,
                    style: 0,
                };
                new_cols
            ],
        );
        for row in &mut self.grid {
            row.resize(
                new_cols,
                Cell {
                    c: ' ',
                    fg: self.current_fg,
                    bg: self.current_bg,
                    style: 0,
                },
            );
        }
        self.cols = new_cols;
        self.rows = new_rows;
    }

    fn empty_cell(&self) -> Cell {
        Cell {
            c: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            style: 0,
        }
    }

    fn empty_row(&self) -> Vec<Cell> {
        vec![self.empty_cell(); self.cols]
    }

    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n {
            if let Some(old_row) = self.grid.pop_front() {
                self.scrollback.push_back(old_row);
            }
            if self.scrollback.len() > MAX_SCROLLBACK {
                self.scrollback.pop_front();
            }
            self.grid.push_back(self.empty_row());
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n {
            self.grid.pop_back();
            self.grid.push_front(self.empty_row());
        }
    }
}

impl Perform for Performer {
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let params_vec: Vec<&[u16]> = params.iter().collect();
        let p0 = params_vec
            .get(0)
            .and_then(|s| s.first())
            .copied()
            .unwrap_or(0) as usize;
        let p1 = params_vec
            .get(1)
            .and_then(|s| s.first())
            .copied()
            .unwrap_or(0) as usize;

        match action {
            'A' => self.cursor_y = self.cursor_y.saturating_sub(p0.max(1)),
            'B' => self.cursor_y = (self.cursor_y + p0.max(1)).min(self.rows - 1),
            'C' => self.cursor_x = (self.cursor_x + p0.max(1)).min(self.cols - 1),
            'D' => self.cursor_x = self.cursor_x.saturating_sub(p0.max(1)),
            'E' => {
                self.cursor_y = (self.cursor_y + p0.max(1)).min(self.rows - 1);
                self.cursor_x = 0;
            }
            'F' => {
                self.cursor_y = self.cursor_y.saturating_sub(p0.max(1));
                self.cursor_x = 0;
            }
            'G' => self.cursor_x = (p0.max(1) - 1).min(self.cols - 1),
            'H' | 'f' => {
                self.cursor_y = (p0.max(1) - 1).min(self.rows - 1);
                self.cursor_x = (p1.max(1) - 1).min(self.cols - 1);
            }
            'd' => self.cursor_y = (p0.max(1) - 1).min(self.rows - 1),

            'J' => {
                let empty = self.empty_cell();
                match p0 {
                    0 => {
                        if self.cursor_y < self.grid.len() {
                            for x in self.cursor_x..self.cols.min(self.grid[self.cursor_y].len()) {
                                self.grid[self.cursor_y][x] = empty;
                            }
                        }
                        for y in (self.cursor_y + 1)..self.grid.len() {
                            self.grid[y] = self.empty_row();
                        }
                    }
                    1 => {
                        for y in 0..self.cursor_y.min(self.grid.len()) {
                            self.grid[y] = self.empty_row();
                        }
                        if self.cursor_y < self.grid.len() {
                            for x in 0..=self
                                .cursor_x
                                .min(self.grid[self.cursor_y].len().saturating_sub(1))
                            {
                                self.grid[self.cursor_y][x] = empty;
                            }
                        }
                    }
                    2 | 3 => {
                        for y in 0..self.grid.len() {
                            self.grid[y] = self.empty_row();
                        }
                        self.cursor_x = 0;
                        self.cursor_y = 0;
                    }
                    _ => {}
                }
            }

            'K' => {
                let empty = self.empty_cell();
                match p0 {
                    0 => {
                        for x in self.cursor_x..self.cols {
                            self.grid[self.cursor_y][x] = empty;
                        }
                    }
                    1 => {
                        for x in 0..=self.cursor_x {
                            self.grid[self.cursor_y][x] = empty;
                        }
                    }
                    2 => self.grid[self.cursor_y] = self.empty_row(),
                    _ => {}
                }
            }

            'X' => {
                let empty = self.empty_cell();
                for x in self.cursor_x..(self.cursor_x + p0.max(1)).min(self.cols) {
                    self.grid[self.cursor_y][x] = empty;
                }
            }

            'S' => self.scroll_up(p0.max(1)),
            'T' => self.scroll_down(p0.max(1)),

            'L' => {
                for _ in 0..p0.max(1) {
                    if self.grid.len() >= self.rows {
                        self.grid.pop_back();
                    }
                    self.grid.insert(self.cursor_y, self.empty_row());
                }
            }
            'M' => {
                for _ in 0..p0.max(1) {
                    if self.cursor_y < self.grid.len() {
                        self.grid.remove(self.cursor_y);
                        self.grid.push_back(self.empty_row());
                    }
                }
            }
            'P' => {
                let empty = self.empty_cell();
                let row = &mut self.grid[self.cursor_y];
                for _ in 0..p0.max(1) {
                    if self.cursor_x < row.len() {
                        row.remove(self.cursor_x);
                        row.push(empty);
                    }
                }
            }
            '@' => {
                let empty = self.empty_cell();
                let row = &mut self.grid[self.cursor_y];
                for _ in 0..p0.max(1) {
                    if row.len() >= self.cols {
                        row.pop();
                    }
                    row.insert(self.cursor_x, empty);
                }
            }

            'q' => {
                match p0 {
                    0 | 1 | 2 => self.cursor_style = CursorStyle::Block,
                    3 | 4 => self.cursor_style = CursorStyle::Underline,
                    5 | 6 => self.cursor_style = CursorStyle::Bar,
                    _ => {}
                }
                self.cursor_blinking = matches!(p0, 1 | 3 | 5);
            }

            'm' => {
                let mut i = 0;
                while i < params_vec.len() {
                    match params_vec[i] {
                        [0] | [] => {
                            self.current_fg = Color::rgb(229, 229, 229);
                            self.current_bg = Color::rgb(0, 0, 0);
                            self.current_style = 0;
                        }
                        [1] => self.current_style |= style::BOLD,
                        [2] => self.current_style |= style::DIM,
                        [3] => self.current_style |= style::ITALIC,
                        [4] => self.current_style |= style::UNDERLINE,
                        [5] | [6] => self.current_style |= style::BLINK,
                        [7] => self.current_style |= style::REVERSE,
                        [8] => self.current_style |= style::HIDDEN,
                        [9] => self.current_style |= style::STRIKETHROUGH,
                        [21] | [22] => self.current_style &= !(style::BOLD | style::DIM),
                        [23] => self.current_style &= !style::ITALIC,
                        [24] => self.current_style &= !style::UNDERLINE,
                        [25] => self.current_style &= !style::BLINK,
                        [27] => self.current_style &= !style::REVERSE,
                        [28] => self.current_style &= !style::HIDDEN,
                        [29] => self.current_style &= !style::STRIKETHROUGH,
                        [30] => self.current_fg = Color::rgb(0, 0, 0),
                        [31] => self.current_fg = Color::rgb(205, 0, 0),
                        [32] => self.current_fg = Color::rgb(0, 205, 0),
                        [33] => self.current_fg = Color::rgb(205, 205, 0),
                        [34] => self.current_fg = Color::rgb(0, 0, 238),
                        [35] => self.current_fg = Color::rgb(205, 0, 205),
                        [36] => self.current_fg = Color::rgb(0, 205, 205),
                        [37] => self.current_fg = Color::rgb(229, 229, 229),
                        [39] => self.current_fg = Color::rgb(229, 229, 229),
                        [38, 5, n] => self.current_fg = color_from_256(*n as u8),
                        [38, 2, r, g, b] => {
                            self.current_fg = Color::rgb(*r as u8, *g as u8, *b as u8)
                        }
                        [38] => {
                            if i + 1 < params_vec.len() {
                                match params_vec[i + 1] {
                                    [5] if i + 2 < params_vec.len() => {
                                        if let [n] = params_vec[i + 2] {
                                            self.current_fg = color_from_256(*n as u8);
                                            i += 2;
                                        }
                                    }
                                    [2] if i + 4 < params_vec.len() => {
                                        if let ([r], [g], [b]) = (
                                            params_vec[i + 2],
                                            params_vec[i + 3],
                                            params_vec[i + 4],
                                        ) {
                                            self.current_fg =
                                                Color::rgb(*r as u8, *g as u8, *b as u8);
                                            i += 4;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        [40] => self.current_bg = Color::rgb(0, 0, 0),
                        [41] => self.current_bg = Color::rgb(205, 0, 0),
                        [42] => self.current_bg = Color::rgb(0, 205, 0),
                        [43] => self.current_bg = Color::rgb(205, 205, 0),
                        [44] => self.current_bg = Color::rgb(0, 0, 238),
                        [45] => self.current_bg = Color::rgb(205, 0, 205),
                        [46] => self.current_bg = Color::rgb(0, 205, 205),
                        [47] => self.current_bg = Color::rgb(229, 229, 229),
                        [49] => self.current_bg = Color::rgb(0, 0, 0),
                        [48, 5, n] => self.current_bg = color_from_256(*n as u8),
                        [48, 2, r, g, b] => {
                            self.current_bg = Color::rgb(*r as u8, *g as u8, *b as u8)
                        }
                        [48] => {
                            if i + 1 < params_vec.len() {
                                match params_vec[i + 1] {
                                    [5] if i + 2 < params_vec.len() => {
                                        if let [n] = params_vec[i + 2] {
                                            self.current_bg = color_from_256(*n as u8);
                                            i += 2;
                                        }
                                    }
                                    [2] if i + 4 < params_vec.len() => {
                                        if let ([r], [g], [b]) = (
                                            params_vec[i + 2],
                                            params_vec[i + 3],
                                            params_vec[i + 4],
                                        ) {
                                            self.current_bg =
                                                Color::rgb(*r as u8, *g as u8, *b as u8);
                                            i += 4;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        [90] => self.current_fg = Color::rgb(127, 127, 127),
                        [91] => self.current_fg = Color::rgb(255, 85, 85),
                        [92] => self.current_fg = Color::rgb(85, 255, 85),
                        [93] => self.current_fg = Color::rgb(255, 255, 85),
                        [94] => self.current_fg = Color::rgb(85, 85, 255),
                        [95] => self.current_fg = Color::rgb(255, 85, 255),
                        [96] => self.current_fg = Color::rgb(85, 255, 255),
                        [97] => self.current_fg = Color::rgb(255, 255, 255),
                        [100] => self.current_bg = Color::rgb(127, 127, 127),
                        [101] => self.current_bg = Color::rgb(255, 85, 85),
                        [102] => self.current_bg = Color::rgb(85, 255, 85),
                        [103] => self.current_bg = Color::rgb(255, 255, 85),
                        [104] => self.current_bg = Color::rgb(85, 85, 255),
                        [105] => self.current_bg = Color::rgb(255, 85, 255),
                        [106] => self.current_bg = Color::rgb(85, 255, 255),
                        [107] => self.current_bg = Color::rgb(255, 255, 255),
                        _ => {}
                    }
                    i += 1;
                }
            }
            _ => {}
        }
    }

    fn print(&mut self, c: char) {
        if self.cursor_y < self.rows && self.cursor_x < self.cols {
            self.grid[self.cursor_y][self.cursor_x] = Cell {
                c,
                fg: self.current_fg,
                bg: self.current_bg,
                style: self.current_style,
            };
            self.cursor_x += 1;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll_up(1);
                    self.cursor_y = self.rows - 1;
                }
            }
            b'\r' => self.cursor_x = 0,
            b'\t' => {
                let tab_width = 8;
                let next_tab_stop = ((self.cursor_x / tab_width) + 1) * tab_width;
                self.cursor_x = if next_tab_stop < self.cols {
                    next_tab_stop
                } else {
                    self.cols - 1
                };
            }
            8 | 127 => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            _ => {}
        }
    }
}

fn color_from_256(n: u8) -> Color {
    match n {
        0 => Color::rgb(0, 0, 0),
        1 => Color::rgb(205, 0, 0),
        2 => Color::rgb(0, 205, 0),
        3 => Color::rgb(205, 205, 0),
        4 => Color::rgb(0, 0, 238),
        5 => Color::rgb(205, 0, 205),
        6 => Color::rgb(0, 205, 205),
        7 => Color::rgb(229, 229, 229),
        8 => Color::rgb(127, 127, 127),
        9 => Color::rgb(255, 85, 85),
        10 => Color::rgb(85, 255, 85),
        11 => Color::rgb(255, 255, 85),
        12 => Color::rgb(85, 85, 255),
        13 => Color::rgb(255, 85, 255),
        14 => Color::rgb(85, 255, 255),
        15 => Color::rgb(255, 255, 255),
        16..=231 => {
            let n = n - 16;
            let b = n % 6;
            let g = (n / 6) % 6;
            let r = n / 36;
            let to_val = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color::rgb(to_val(r), to_val(g), to_val(b))
        }
        232..=255 => {
            let v = 8 + (n - 232) * 10;
            Color::rgb(v, v, v)
        }
    }
}

#[test]
fn test_bold_flag() {
    let mut term = Terminal::new();
    term.process(b"\x1b[1mA");
    let cell = term.performer.grid[0][0];
    assert!(cell.style & style::BOLD != 0);
}

#[test]
fn test_reset_bold() {
    let mut term = Terminal::new();
    term.process(b"\x1b[1mA\x1b[22mB");
    let a = term.performer.grid[0][0];
    let b = term.performer.grid[0][1];
    assert!(a.style & style::BOLD != 0);
    assert!(b.style & style::BOLD == 0);
}

#[test]
fn test_color() {
    let mut term = Terminal::new();
    term.process(b"\x1b[31mR\x1b[32mG");
    let r = term.performer.grid[0][0];
    let g = term.performer.grid[0][1];
    assert_eq!(r.fg, Color::rgb(205, 0, 0));
    assert_eq!(g.fg, Color::rgb(0, 205, 0));
}
