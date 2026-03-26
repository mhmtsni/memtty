use std::{collections::VecDeque, fmt::Debug};

use iced::Color;
use vte::{Parser, Perform};

const MAX_SCROLLBACK: usize = 1000;
const ROWS: usize = 48;
const COLS: usize = 160;

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::WHITE,
            bg: Color::BLACK,
        }
    }
}

#[derive(Default)]
pub struct Performer {
    pub grid: VecDeque<Vec<Cell>>,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub cols: usize,
    pub rows: usize,
    pub buffer: String,
    pub current_fg: Color,
    pub current_bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub dim: bool,
    pub blink: bool,
    pub reverse: bool,
    pub hidden: bool,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: bool,
    pub cursor_visible: bool,
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
    pub buffer: String,
    pub parser: Parser,
    pub performer: Performer,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            parser: Parser::new(),
            performer: Performer {
                grid: VecDeque::from(vec![vec![Cell::default(); COLS]; ROWS]),
                scrollback: VecDeque::new(),
                cursor_x: 0,

                cursor_style: CursorStyle::Block,
                cursor_blinking: false,
                cursor_visible: true,
                cursor_y: 0,
                cols: COLS,
                rows: ROWS,
                buffer: String::new(),
                current_fg: Color::WHITE,
                current_bg: Color::BLACK,
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                dim: false,
                blink: false,
                reverse: false,
                hidden: false,
            },
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.performer, bytes);
        self.buffer.push_str(&self.performer.buffer);
        self.performer.buffer.clear();
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
                },
            );
        }
        self.cols = new_cols;
        self.rows = new_rows;
    }

    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n {
            if let Some(old_row) = self.grid.pop_front() {
                self.scrollback.push_back(old_row);
            };
            if self.scrollback.len() > MAX_SCROLLBACK {
                self.scrollback.pop_front();
            }
            self.grid.push_back(vec![
                Cell {
                    c: ' ',
                    fg: self.current_fg,
                    bg: self.current_bg,
                };
                self.cols
            ]);
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n {
            self.grid.pop_back();
            self.grid.push_front(vec![
                Cell {
                    c: ' ',
                    fg: self.current_fg,
                    bg: self.current_bg,
                };
                self.cols
            ]);
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
            // -------------------------------------------------------
            // Cursor Movement
            // -------------------------------------------------------
            'A' => {
                // CUU - Cursor Up
                let n = p0.max(1);
                self.cursor_y = self.cursor_y.saturating_sub(n);
            }
            'B' => {
                // CUD - Cursor Down
                let n = p0.max(1);
                self.cursor_y = (self.cursor_y + n).min(self.rows - 1);
            }
            'C' => {
                // CUF - Cursor Forward
                let n = p0.max(1);
                self.cursor_x = (self.cursor_x + n).min(self.cols - 1);
            }
            'D' => {
                // CUB - Cursor Back
                let n = p0.max(1);
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            'E' => {
                // CNL - Cursor Next Line
                let n = p0.max(1);
                self.cursor_y = (self.cursor_y + n).min(self.rows - 1);
                self.cursor_x = 0;
            }
            'F' => {
                // CPL - Cursor Previous Line
                let n = p0.max(1);
                self.cursor_y = self.cursor_y.saturating_sub(n);
                self.cursor_x = 0;
            }
            'G' => {
                // CHA - Cursor Horizontal Absolute (1-based)
                let n = p0.max(1);
                self.cursor_x = (n - 1).min(self.cols - 1);
            }
            'H' | 'f' => {
                // CUP / HVP - Cursor Position (1-based)
                let row = p0.max(1) - 1;
                let col = p1.max(1) - 1;
                self.cursor_y = row.min(self.rows - 1);
                self.cursor_x = col.min(self.cols - 1);
            }
            'd' => {
                // VPA - Vertical Line Position Absolute (1-based)
                let n = p0.max(1) - 1;
                self.cursor_y = n.min(self.rows - 1);
            }

            // -------------------------------------------------------
            // Erase
            // -------------------------------------------------------
            'J' => {
                // ED - Erase in Display
                match p0 {
                    0 => {
                        // Cursor to end of screen
                        for x in self.cursor_x..self.cols {
                            self.grid[self.cursor_y][x] = Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                        }
                        for y in (self.cursor_y + 1)..self.rows {
                            self.grid[y] = vec![
                                Cell {
                                    c: ' ',
                                    fg: self.current_fg,
                                    bg: self.current_bg,
                                };
                                self.cols
                            ];
                        }
                    }
                    1 => {
                        // Start of screen to cursor
                        for y in 0..self.cursor_y {
                            self.grid[y] = vec![
                                Cell {
                                    c: ' ',
                                    fg: self.current_fg,
                                    bg: self.current_bg,
                                };
                                self.cols
                            ];
                        }
                        for x in 0..=self.cursor_x {
                            self.grid[self.cursor_y][x] = Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                        }
                    }
                    2 | 3 => {
                        // Entire screen
                        for y in 0..self.rows {
                            self.grid[y] = vec![
                                Cell {
                                    c: ' ',
                                    fg: self.current_fg,
                                    bg: self.current_bg,
                                };
                                self.cols
                            ];
                        }
                        self.cursor_x = 0;
                        self.cursor_y = 0;
                    }
                    _ => {}
                }
            }
            'K' => {
                // EL - Erase in Line
                match p0 {
                    0 => {
                        // Cursor to end of line
                        for x in self.cursor_x..self.cols {
                            self.grid[self.cursor_y][x] = Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                        }
                    }
                    1 => {
                        // Start of line to cursor
                        for x in 0..=self.cursor_x {
                            self.grid[self.cursor_y][x] = Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                        }
                    }
                    2 => {
                        // Entire line
                        self.grid[self.cursor_y] = vec![
                            Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                            self.cols
                        ];
                    }
                    _ => {}
                }
            }
            'X' => {
                // ECH - Erase Character
                let n = p0.max(1);
                for x in self.cursor_x..(self.cursor_x + n).min(self.cols) {
                    self.grid[self.cursor_y][x] = Cell {
                        c: ' ',
                        fg: self.current_fg,
                        bg: self.current_bg,
                    };
                }
            }

            // -------------------------------------------------------
            // Scroll
            // -------------------------------------------------------
            'S' => {
                // SU - Scroll Up
                let n = p0.max(1);
                self.scroll_up(n);
            }
            'T' => {
                // SD - Scroll Down
                let n = p0.max(1);
                self.scroll_down(n);
            }

            // -------------------------------------------------------
            // Insert / Delete
            // -------------------------------------------------------
            'L' => {
                // IL - Insert Line
                let n = p0.max(1);
                for _ in 0..n {
                    if self.grid.len() >= self.rows {
                        self.grid.pop_back();
                    }
                    self.grid.insert(
                        self.cursor_y,
                        vec![
                            Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                            self.cols
                        ],
                    );
                }
            }
            'M' => {
                // DL - Delete Line
                let n = p0.max(1);
                for _ in 0..n {
                    if self.cursor_y < self.grid.len() {
                        self.grid.remove(self.cursor_y);
                        self.grid.push_back(vec![
                            Cell {
                                c: ' ',
                                fg: self.current_fg,
                                bg: self.current_bg,
                            };
                            self.cols
                        ]);
                    }
                }
            }
            'P' => {
                // DCH - Delete Character
                let n = p0.max(1);
                let row = &mut self.grid[self.cursor_y];
                for _ in 0..n {
                    if self.cursor_x < row.len() {
                        row.remove(self.cursor_x);
                        row.push(Cell {
                            c: ' ',
                            fg: self.current_fg,
                            bg: self.current_bg,
                        });
                    }
                }
            }
            '@' => {
                // ICH - Insert Character
                let n = p0.max(1);
                let row = &mut self.grid[self.cursor_y];
                for _ in 0..n {
                    if row.len() >= self.cols {
                        row.pop();
                    }
                    row.insert(
                        self.cursor_x,
                        Cell {
                            c: ' ',
                            fg: self.current_fg,
                            bg: self.current_bg,
                        },
                    );
                }
            }

            'q' => {
                let mode = p0;
                match mode {
                    0 | 1 | 2 => self.cursor_style = CursorStyle::Block,
                    3 | 4 => self.cursor_style = CursorStyle::Underline,
                    5 | 6 => self.cursor_style = CursorStyle::Bar,
                    _ => {}
                }

                self.cursor_blinking = matches!(mode, 1 | 3 | 5);
            }

            // -------------------------------------------------------
            // SGR - Select Graphic Rendition (colors & attributes)
            // -------------------------------------------------------
            'm' => {
                let mut i = 0;
                while i < params_vec.len() {
                    match params_vec[i] {
                        [0] | [] => {
                            self.current_fg = Color::from_rgb8(229, 229, 229);
                            self.current_bg = Color::from_rgb8(0, 0, 0);
                            self.bold = false;
                            self.italic = false;
                            self.underline = false;
                            self.strikethrough = false;
                            self.dim = false;
                            self.blink = false;
                            self.reverse = false;
                            self.hidden = false;
                        }

                        // --- Text Attributes ---
                        [1] => self.bold = true,
                        [2] => self.dim = true,
                        [3] => self.italic = true,
                        [4] => self.underline = true,
                        [5] | [6] => self.blink = true,
                        [7] => self.reverse = true,
                        [8] => self.hidden = true,
                        [9] => self.strikethrough = true,

                        // --- Attribute Resets ---
                        [21] | [22] => {
                            self.bold = false;
                            self.dim = false;
                        }
                        [23] => self.italic = false,
                        [24] => self.underline = false,
                        [25] => self.blink = false,
                        [27] => self.reverse = false,
                        [28] => self.hidden = false,
                        [29] => self.strikethrough = false,

                        // --- Standard Foreground Colors (30-37) ---
                        [30] => self.current_fg = Color::from_rgb8(0, 0, 0),
                        [31] => self.current_fg = Color::from_rgb8(205, 0, 0),
                        [32] => self.current_fg = Color::from_rgb8(0, 205, 0),
                        [33] => self.current_fg = Color::from_rgb8(205, 205, 0),
                        [34] => self.current_fg = Color::from_rgb8(0, 0, 238),
                        [35] => self.current_fg = Color::from_rgb8(205, 0, 205),
                        [36] => self.current_fg = Color::from_rgb8(0, 205, 205),
                        [39] => self.current_fg = Color::from_rgb8(229, 229, 229),

                        // --- 256-color / Truecolor Foreground (38) ---
                        [38, 5, n] => {
                            self.current_fg = color_from_256(*n as u8);
                        }
                        [38, 2, r, g, b] => {
                            self.current_fg = Color::from_rgb8(*r as u8, *g as u8, *b as u8);
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
                                                Color::from_rgb8(*r as u8, *g as u8, *b as u8);
                                            i += 4;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // --- Standard Background Colors (40-47) ---
                        [40] => self.current_bg = Color::from_rgb8(0, 0, 0),
                        [41] => self.current_bg = Color::from_rgb8(205, 0, 0),
                        [42] => self.current_bg = Color::from_rgb8(0, 205, 0),
                        [43] => self.current_bg = Color::from_rgb8(205, 205, 0),
                        [44] => self.current_bg = Color::from_rgb8(0, 0, 238),
                        [45] => self.current_bg = Color::from_rgb8(205, 0, 205),
                        [46] => self.current_bg = Color::from_rgb8(0, 205, 205),
                        [47] => self.current_bg = Color::from_rgb8(229, 229, 229),
                        [49] => self.current_bg = Color::from_rgb8(0, 0, 0),

                        // --- 256-color / Truecolor Background (48) ---
                        [48, 5, n] => {
                            self.current_bg = color_from_256(*n as u8);
                        }
                        [48, 2, r, g, b] => {
                            self.current_bg = Color::from_rgb8(*r as u8, *g as u8, *b as u8);
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
                                                Color::from_rgb8(*r as u8, *g as u8, *b as u8);
                                            i += 4;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // --- Bright Foreground Colors (90-97) ---
                        [90] => self.current_fg = Color::from_rgb8(127, 127, 127),
                        [91] => self.current_fg = Color::from_rgb8(255, 85, 85),
                        [92] => self.current_fg = Color::from_rgb8(85, 255, 85),
                        [93] => self.current_fg = Color::from_rgb8(255, 255, 85),
                        [94] => self.current_fg = Color::from_rgb8(85, 85, 255),
                        [95] => self.current_fg = Color::from_rgb8(255, 85, 255),
                        [96] => self.current_fg = Color::from_rgb8(85, 255, 255),
                        [97] => self.current_fg = Color::from_rgb8(255, 255, 255),

                        // --- Bright Background Colors (100-107) ---
                        [100] => self.current_bg = Color::from_rgb8(127, 127, 127),
                        [101] => self.current_bg = Color::from_rgb8(255, 85, 85),
                        [102] => self.current_bg = Color::from_rgb8(85, 255, 85),
                        [103] => self.current_bg = Color::from_rgb8(255, 255, 85),
                        [104] => self.current_bg = Color::from_rgb8(85, 85, 255),
                        [105] => self.current_bg = Color::from_rgb8(255, 85, 255),
                        [106] => self.current_bg = Color::from_rgb8(85, 255, 255),
                        [107] => self.current_bg = Color::from_rgb8(255, 255, 255),

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
                if next_tab_stop < self.cols {
                    self.cursor_x = next_tab_stop;
                } else {
                    self.cursor_x = self.cols - 1;
                }
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
        // --- 0-7: Standard colors ---
        0 => Color::from_rgb8(0, 0, 0),
        1 => Color::from_rgb8(205, 0, 0),
        2 => Color::from_rgb8(0, 205, 0),
        3 => Color::from_rgb8(205, 205, 0),
        4 => Color::from_rgb8(0, 0, 238),
        5 => Color::from_rgb8(205, 0, 205),
        6 => Color::from_rgb8(0, 205, 205),
        7 => Color::from_rgb8(229, 229, 229),

        // --- 8-15: Bright colors ---
        8 => Color::from_rgb8(127, 127, 127),
        9 => Color::from_rgb8(255, 85, 85),
        10 => Color::from_rgb8(85, 255, 85),
        11 => Color::from_rgb8(255, 255, 85),
        12 => Color::from_rgb8(85, 85, 255),
        13 => Color::from_rgb8(255, 85, 255),
        14 => Color::from_rgb8(85, 255, 255),
        15 => Color::from_rgb8(255, 255, 255),

        // --- 16-231: 6x6x6 color cube ---
        16..=231 => {
            let n = n - 16;
            let b = n % 6;
            let g = (n / 6) % 6;
            let r = n / 36;
            let to_val = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color::from_rgb8(to_val(r), to_val(g), to_val(b))
        }

        // --- 232-255: Grayscale ramp ---
        232..=255 => {
            let v = 8 + (n - 232) * 10;
            Color::from_rgb8(v, v, v)
        }
    }
}
