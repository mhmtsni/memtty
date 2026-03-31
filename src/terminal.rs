use std::collections::VecDeque;

use glyphon::Color;
use vte::{Parser, Perform};

const MAX_SCROLLBACK: usize = 1000;
const ROWS: usize = 24;
const COLS: usize = 80;
const DEFAULT_FG: Color = Color::rgb(229, 229, 229);
const DEFAULT_BG: Color = Color::rgb(33, 38, 52);

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
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            style: 0,
        }
    }
}

/// Saved cursor state (DECSC/DECRC and CSI s/u).
#[derive(Clone, Copy)]
struct SavedCursor {
    x: usize,
    y: usize,
    fg: Color,
    bg: Color,
    style: u8,
    origin_mode: bool,
    g0_charset: Charset,
    g1_charset: Charset,
    use_g1_charset: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Ascii,
    DecSpecialGraphics,
}

#[derive(Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

pub struct Performer {
    // ── buffers ──────────────────────────────────────────────────────────────
    pub grid: VecDeque<Vec<Cell>>,
    pub scrollback: VecDeque<Vec<Cell>>,

    // alt-screen state (swapped in/out by ?1049h / ?1049l)
    alt_grid: Option<VecDeque<Vec<Cell>>>,
    alt_cursor: Option<SavedCursor>,
    pub in_alt_screen: bool,

    // ── cursor ────────────────────────────────────────────────────────────────
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: bool,
    pub cursor_visible: bool,

    saved_cursor: Option<SavedCursor>,

    // ── dimensions ───────────────────────────────────────────────────────────
    pub cols: usize,
    pub rows: usize,

    // ── scroll region (inclusive, 0-based) ───────────────────────────────────
    scroll_top: usize,
    scroll_bottom: usize,

    // ── SGR state ────────────────────────────────────────────────────────────
    default_fg: Color,
    default_bg: Color,
    palette_256: [Color; 256],
    pub current_fg: Color,
    pub current_bg: Color,
    pub current_style: u8,

    // ── modes ─────────────────────────────────────────────────────────────────
    /// Auto-wrap mode (default on).
    auto_wrap: bool,
    /// DEC origin mode: cursor movement is relative to scroll region.
    origin_mode: bool,
    /// Application cursor keys (DECCKM).
    pub app_cursor_keys: bool,
    /// Bracketed paste mode.
    pub bracketed_paste: bool,
    /// Mouse tracking modes.
    pub mouse_mode: MouseMode,

    // G0/G1 charset designations and active GL selection (SI/SO).
    g0_charset: Charset,
    g1_charset: Charset,
    use_g1_charset: bool,

    // pending wrap: next print will first advance to next line
    pending_wrap: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum MouseMode {
    #[default]
    None,
    X10,         // ?9
    Normal,      // ?1000
    ButtonEvent, // ?1002
    AnyEvent,    // ?1003
}

#[derive(Default)]
pub struct Terminal {
    pub parser: Parser,
    pub performer: Performer,
}

// ─── Default / constructor ────────────────────────────────────────────────────

impl Default for Performer {
    fn default() -> Self {
        let rows = ROWS;
        let cols = COLS;
        let palette_256 = default_palette_256();
        let default_fg = DEFAULT_FG;
        let default_bg = DEFAULT_BG;
        let default_cell = Cell {
            c: ' ',
            fg: default_fg,
            bg: default_bg,
            style: 0,
        };
        Self {
            grid: VecDeque::from(vec![vec![default_cell; cols]; rows]),
            scrollback: VecDeque::new(),
            alt_grid: None,
            alt_cursor: None,
            in_alt_screen: false,
            cursor_x: 0,
            cursor_y: 0,
            cursor_style: CursorStyle::Block,
            cursor_blinking: false,
            cursor_visible: true,
            saved_cursor: None,
            cols,
            rows,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            default_fg,
            default_bg,
            palette_256,
            current_fg: default_fg,
            current_bg: default_bg,
            current_style: 0,
            auto_wrap: true,
            origin_mode: false,
            app_cursor_keys: false,
            bracketed_paste: false,
            mouse_mode: MouseMode::None,
            g0_charset: Charset::Ascii,
            g1_charset: Charset::Ascii,
            use_g1_charset: false,
            pending_wrap: false,
        }
    }
}

// ─── Terminal public API ──────────────────────────────────────────────────────

impl Terminal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.performer, bytes);
    }

    /// Visible rows for rendering, honouring scroll offset (positive = scrolled up).
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

// ─── Performer helpers ────────────────────────────────────────────────────────

impl Performer {
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        self.cols = new_cols;
        self.rows = new_rows;
        self.normalize_grid_dimensions();
        self.scroll_top = 0;
        self.scroll_bottom = new_rows - 1;
        self.clamp_cursor();
    }

    fn normalize_grid_dimensions(&mut self) {
        let blank = Cell {
            c: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            style: 0,
        };
        self.grid.resize(self.rows, vec![blank; self.cols]);
        for row in &mut self.grid {
            row.resize(self.cols, blank);
        }
    }

    fn clamp_cursor(&mut self) {
        let (y_max, x_max) = if self.origin_mode {
            (self.scroll_bottom, self.cols - 1)
        } else {
            (self.rows - 1, self.cols - 1)
        };
        self.cursor_x = self.cursor_x.min(x_max);
        self.cursor_y = self.cursor_y.min(y_max);
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

    // ── scroll-region scroll ──────────────────────────────────────────────────

    /// Scroll the scroll region up by `n` lines (content moves up, new blank
    /// lines appear at the bottom of the region).
    fn scroll_up_region(&mut self, n: usize) {
        for _ in 0..n {
            // Only push to scrollback when the region covers the top of the screen
            if self.scroll_top == 0 {
                if let Some(old_row) = self.grid.pop_front() {
                    self.scrollback.push_back(old_row);
                    if self.scrollback.len() > MAX_SCROLLBACK {
                        self.scrollback.pop_front();
                    }
                    self.grid.push_back(self.empty_row()); // push back at real bottom
                } else {
                    continue;
                }
                // Now remove the row that fell into the region's bottom from the
                // right place if scroll_bottom < rows-1 – handled generically below,
                // but because pop_front already shifts everything, the generic path
                // would double-shift. For full-screen regions we're done.
            } else {
                // Generic: remove top row of region, insert blank at bottom.
                if self.scroll_top < self.grid.len() {
                    self.grid.remove(self.scroll_top);
                }
                let ins = (self.scroll_bottom).min(self.grid.len());
                self.grid.insert(ins, self.empty_row());
            }
        }
    }

    /// Scroll the scroll region down by `n` lines.
    fn scroll_down_region(&mut self, n: usize) {
        for _ in 0..n {
            if self.scroll_bottom < self.grid.len() {
                self.grid.remove(self.scroll_bottom);
            }
            self.grid.insert(self.scroll_top, self.empty_row());
        }
    }

    // kept for CSI S / T (which scroll the whole visible area)
    fn scroll_up(&mut self, n: usize) {
        let saved_top = self.scroll_top;
        let saved_bot = self.scroll_bottom;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.scroll_up_region(n);
        self.scroll_top = saved_top;
        self.scroll_bottom = saved_bot;
    }

    fn scroll_down(&mut self, n: usize) {
        let saved_top = self.scroll_top;
        let saved_bot = self.scroll_bottom;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.scroll_down_region(n);
        self.scroll_top = saved_top;
        self.scroll_bottom = saved_bot;
    }

    // ── cursor save / restore ─────────────────────────────────────────────────

    fn save_cursor(&mut self) {
        self.saved_cursor = Some(SavedCursor {
            x: self.cursor_x,
            y: self.cursor_y,
            fg: self.current_fg,
            bg: self.current_bg,
            style: self.current_style,
            origin_mode: self.origin_mode,
            g0_charset: self.g0_charset,
            g1_charset: self.g1_charset,
            use_g1_charset: self.use_g1_charset,
        });
    }

    fn restore_cursor(&mut self) {
        if let Some(s) = self.saved_cursor {
            self.cursor_x = s.x.min(self.cols - 1);
            self.cursor_y = s.y.min(self.rows - 1);
            self.current_fg = s.fg;
            self.current_bg = s.bg;
            self.current_style = s.style;
            self.origin_mode = s.origin_mode;
            self.g0_charset = s.g0_charset;
            self.g1_charset = s.g1_charset;
            self.use_g1_charset = s.use_g1_charset;
            self.pending_wrap = false;
        }
    }

    // ── alt screen ────────────────────────────────────────────────────────────

    fn enter_alt_screen(&mut self) {
        if self.in_alt_screen {
            return;
        }
        // Save normal screen
        let blank = Cell {
            c: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            style: 0,
        };
        self.alt_grid = Some(std::mem::replace(
            &mut self.grid,
            VecDeque::from(vec![vec![blank; self.cols]; self.rows]),
        ));
        self.alt_cursor = Some(SavedCursor {
            x: self.cursor_x,
            y: self.cursor_y,
            fg: self.current_fg,
            bg: self.current_bg,
            style: self.current_style,
            origin_mode: self.origin_mode,
            g0_charset: self.g0_charset,
            g1_charset: self.g1_charset,
            use_g1_charset: self.use_g1_charset,
        });
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.in_alt_screen = true;
    }

    fn exit_alt_screen(&mut self) {
        if !self.in_alt_screen {
            return;
        }
        if let Some(grid) = self.alt_grid.take() {
            self.grid = grid;
            // If window size changed while in alt-screen, restore content and then
            // force dimensions to the active terminal size to keep indexing safe.
            self.normalize_grid_dimensions();
        }
        if let Some(c) = self.alt_cursor.take() {
            self.cursor_x = c.x.min(self.cols - 1);
            self.cursor_y = c.y.min(self.rows - 1);
            self.current_fg = c.fg;
            self.current_bg = c.bg;
            self.current_style = c.style;
            self.g0_charset = c.g0_charset;
            self.g1_charset = c.g1_charset;
            self.use_g1_charset = c.use_g1_charset;
            self.origin_mode = c.origin_mode;
        }
        self.in_alt_screen = false;
        self.pending_wrap = false;
    }

    fn active_charset(&self) -> Charset {
        if self.use_g1_charset {
            self.g1_charset
        } else {
            self.g0_charset
        }
    }

    fn translate_char(&self, c: char) -> char {
        match self.active_charset() {
            Charset::Ascii => c,
            Charset::DecSpecialGraphics => map_dec_special_graphics(c),
        }
    }

    // ── DEC private mode set/reset ────────────────────────────────────────────

    fn set_dec_mode(&mut self, mode: usize, enable: bool) {
        match mode {
            1 => self.app_cursor_keys = enable,
            6 => {
                // DECOM: origin mode
                self.origin_mode = enable;
                // Cursor moves to home on mode change
                self.cursor_x = 0;
                self.cursor_y = if enable { self.scroll_top } else { 0 };
                self.pending_wrap = false;
            }
            7 => self.auto_wrap = enable,
            9 => {
                self.mouse_mode = if enable {
                    MouseMode::X10
                } else {
                    MouseMode::None
                }
            }
            12 => self.cursor_blinking = enable,
            25 => self.cursor_visible = enable,
            1000 => {
                self.mouse_mode = if enable {
                    MouseMode::Normal
                } else {
                    MouseMode::None
                }
            }
            1002 => {
                self.mouse_mode = if enable {
                    MouseMode::ButtonEvent
                } else {
                    MouseMode::None
                }
            }
            1003 => {
                self.mouse_mode = if enable {
                    MouseMode::AnyEvent
                } else {
                    MouseMode::None
                }
            }
            1049 => {
                if enable {
                    self.save_cursor();
                    self.enter_alt_screen();
                } else {
                    self.exit_alt_screen();
                    self.restore_cursor();
                }
            }
            2004 => self.bracketed_paste = enable,
            _ => {}
        }
    }
}

// ─── vte::Perform implementation ─────────────────────────────────────────────

impl Perform for Performer {
    // ── printable character ───────────────────────────────────────────────────

    fn print(&mut self, c: char) {
        let c = self.translate_char(c);

        if self.pending_wrap && self.auto_wrap {
            self.cursor_x = 0;
            self.cursor_y += 1;
            if self.cursor_y > self.scroll_bottom {
                self.scroll_up_region(1);
                self.cursor_y = self.scroll_bottom;
            }
            self.pending_wrap = false;
        }

        if self.cursor_y < self.grid.len() {
            let row_len = self.grid[self.cursor_y].len();
            if self.cursor_x >= row_len {
                return;
            }

            self.grid[self.cursor_y][self.cursor_x] = Cell {
                c,
                fg: self.current_fg,
                bg: self.current_bg,
                style: self.current_style,
            };

            if self.cursor_x + 1 >= row_len {
                // Reached last column — defer wrap until next print
                self.pending_wrap = true;
            } else {
                self.cursor_x += 1;
            }
        }
    }

    // ── C0 / C1 control codes ─────────────────────────────────────────────────

    fn execute(&mut self, byte: u8) {
        match byte {
            // LF / VT / FF  — newline with scroll
            b'\n' | 0x0B | 0x0C => {
                self.pending_wrap = false;
                if self.cursor_y == self.scroll_bottom {
                    self.scroll_up_region(1);
                } else {
                    self.cursor_y = (self.cursor_y + 1).min(self.rows - 1);
                }
            }
            b'\r' => {
                self.cursor_x = 0;
                self.pending_wrap = false;
            }
            b'\t' => {
                let tab_width = 8;
                let next_tab = ((self.cursor_x / tab_width) + 1) * tab_width;
                self.cursor_x = next_tab.min(self.cols - 1);
                self.pending_wrap = false;
            }
            // BS
            0x08 => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
                self.pending_wrap = false;
            }
            // DEL — ignored
            0x7F => {}
            // BEL — bell (callers can poll cursor_visible / add a bell flag)
            0x07 => {}
            // SO/SI — shift out/in: select G1/G0 into GL.
            0x0E => self.use_g1_charset = true,
            0x0F => self.use_g1_charset = false,
            _ => {}
        }
    }

    // ── ESC sequences (not CSI, not OSC) ─────────────────────────────────────

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates.first().copied(), byte) {
            // DECSC — save cursor
            (None, b'7') => self.save_cursor(),
            // DECRC — restore cursor
            (None, b'8') => self.restore_cursor(),

            // IND — index (like LF, but ignores LNM)
            (None, b'D') => {
                if self.cursor_y == self.scroll_bottom {
                    self.scroll_up_region(1);
                } else {
                    self.cursor_y = (self.cursor_y + 1).min(self.rows - 1);
                }
            }
            // NEL — next line
            (None, b'E') => {
                self.cursor_x = 0;
                if self.cursor_y == self.scroll_bottom {
                    self.scroll_up_region(1);
                } else {
                    self.cursor_y = (self.cursor_y + 1).min(self.rows - 1);
                }
            }
            // RI — reverse index (scroll down if at top of scroll region)
            (None, b'M') => {
                if self.cursor_y == self.scroll_top {
                    self.scroll_down_region(1);
                } else {
                    self.cursor_y = self.cursor_y.saturating_sub(1);
                }
            }
            // HTS — set horizontal tab stop (stub: we use fixed 8-col tabs)
            (None, b'H') => {}

            // RIS — full reset
            (None, b'c') => {
                *self = Performer::default();
            }

            // DECALN — fill screen with 'E' (alignment test)
            (Some(b'#'), b'8') => {
                for row in &mut self.grid {
                    for cell in row.iter_mut() {
                        cell.c = 'E';
                    }
                }
            }

            // Charset designations — G0/G1.
            (Some(b'('), designator) => {
                self.g0_charset = charset_from_designator(designator);
            }
            (Some(b')'), designator) => {
                self.g1_charset = charset_from_designator(designator);
            }

            // G2/G3 designations are currently ignored.
            (Some(b'*'), _) | (Some(b'+'), _) => {}

            _ => {}
        }
    }

    // ── CSI sequences ─────────────────────────────────────────────────────────

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // Flatten params into a simple Vec<u16>.
        let params_vec: Vec<u16> = params
            .iter()
            .map(|sub| sub.first().copied().unwrap_or(0))
            .collect();

        let p = |idx: usize| params_vec.get(idx).copied().unwrap_or(0) as usize;
        let p1 = |idx: usize| p(idx).max(1); // param defaulting to 1

        // Private sequences: CSI ? ...
        if intermediates.first() == Some(&b'?') {
            match action {
                'h' => {
                    for &mode in &params_vec {
                        self.set_dec_mode(mode as usize, true);
                    }
                }
                'l' => {
                    for &mode in &params_vec {
                        self.set_dec_mode(mode as usize, false);
                    }
                }
                // DECTCEM aliases handled inside set_dec_mode (mode 25)
                _ => {}
            }
            return;
        }

        // CSI > — secondary DA / xterm version (respond with nothing, stub)
        if intermediates.first() == Some(&b'>') {
            return;
        }

        match action {
            // ── Cursor movement ───────────────────────────────────────────────
            // CUU — cursor up
            'A' => {
                let n = p1(0);
                self.cursor_y = self.cursor_y.saturating_sub(n).max(self.scroll_top);
                self.pending_wrap = false;
            }
            // CUD — cursor down
            'B' => {
                let n = p1(0);
                self.cursor_y = (self.cursor_y + n).min(self.scroll_bottom);
                self.pending_wrap = false;
            }
            // CUF — cursor forward
            'C' => {
                let n = p1(0);
                self.cursor_x = (self.cursor_x + n).min(self.cols - 1);
                self.pending_wrap = false;
            }
            // CUB — cursor backward
            'D' => {
                let n = p1(0);
                self.cursor_x = self.cursor_x.saturating_sub(n);
                self.pending_wrap = false;
            }
            // CNL — cursor next line
            'E' => {
                let n = p1(0);
                self.cursor_y = (self.cursor_y + n).min(self.rows - 1);
                self.cursor_x = 0;
                self.pending_wrap = false;
            }
            // CPL — cursor previous line
            'F' => {
                let n = p1(0);
                self.cursor_y = self.cursor_y.saturating_sub(n);
                self.cursor_x = 0;
                self.pending_wrap = false;
            }
            // CHA — cursor horizontal absolute
            'G' => {
                self.cursor_x = (p1(0) - 1).min(self.cols - 1);
                self.pending_wrap = false;
            }
            // CUP / HVP — cursor position
            'H' | 'f' => {
                let row = (p1(0) - 1).min(self.rows - 1);
                let col = (p1(1) - 1).min(self.cols - 1);
                self.cursor_y = if self.origin_mode {
                    (self.scroll_top + row).min(self.scroll_bottom)
                } else {
                    row
                };
                self.cursor_x = col;
                self.pending_wrap = false;
            }
            // VPA — vertical line position absolute
            'd' => {
                let row = (p1(0) - 1).min(self.rows - 1);
                self.cursor_y = if self.origin_mode {
                    (self.scroll_top + row).min(self.scroll_bottom)
                } else {
                    row
                };
                self.pending_wrap = false;
            }
            // HPA — horizontal position absolute (same as CHA)
            '`' => {
                self.cursor_x = (p1(0) - 1).min(self.cols - 1);
                self.pending_wrap = false;
            }

            // ── Erase ─────────────────────────────────────────────────────────
            // ED — erase in display
            'J' => {
                let empty = self.empty_cell();
                match p(0) {
                    0 => {
                        // erase from cursor to end of screen
                        if self.cursor_y < self.grid.len() {
                            let row_len = self.grid[self.cursor_y].len();
                            for x in self.cursor_x..row_len {
                                self.grid[self.cursor_y][x] = empty;
                            }
                        }
                        for y in (self.cursor_y + 1)..self.grid.len() {
                            self.grid[y] = self.empty_row();
                        }
                    }
                    1 => {
                        // erase from start to cursor
                        for y in 0..self.cursor_y.min(self.grid.len()) {
                            self.grid[y] = self.empty_row();
                        }
                        if self.cursor_y < self.grid.len() {
                            let end = (self.cursor_x + 1).min(self.grid[self.cursor_y].len());
                            for x in 0..end {
                                self.grid[self.cursor_y][x] = empty;
                            }
                        }
                    }
                    2 | 3 => {
                        // erase whole screen (3 also clears scrollback)
                        if p(0) == 3 {
                            self.scrollback.clear();
                        }
                        let blank_row = self.empty_row();
                        for row in self.grid.iter_mut() {
                            *row = blank_row.clone();
                        }
                        self.cursor_x = 0;
                        self.cursor_y = 0;
                        self.pending_wrap = false;
                    }
                    _ => {}
                }
            }
            // EL — erase in line
            'K' => {
                let empty = self.empty_cell();
                if self.cursor_y >= self.grid.len() {
                    return;
                }
                match p(0) {
                    0 => {
                        // erase to end of line
                        for x in self.cursor_x..self.cols {
                            self.grid[self.cursor_y][x] = empty;
                        }
                    }
                    1 => {
                        // erase to start of line
                        for x in 0..=self.cursor_x.min(self.cols - 1) {
                            self.grid[self.cursor_y][x] = empty;
                        }
                    }
                    2 => self.grid[self.cursor_y] = self.empty_row(),
                    _ => {}
                }
            }
            // ECH — erase character
            'X' => {
                let empty = self.empty_cell();
                let n = p1(0);
                for x in self.cursor_x..(self.cursor_x + n).min(self.cols) {
                    self.grid[self.cursor_y][x] = empty;
                }
            }

            // ── Scroll ────────────────────────────────────────────────────────
            // SU — scroll up
            'S' => self.scroll_up(p1(0)),
            // SD — scroll down
            'T' => self.scroll_down(p1(0)),

            // ── Line insertion / deletion ─────────────────────────────────────
            // IL — insert lines
            'L' => {
                let n = p1(0);
                if self.cursor_y >= self.scroll_top && self.cursor_y <= self.scroll_bottom {
                    for _ in 0..n {
                        if self.scroll_bottom < self.grid.len() {
                            self.grid.remove(self.scroll_bottom);
                        }
                        self.grid.insert(self.cursor_y, self.empty_row());
                    }
                }
                self.cursor_x = 0;
                self.pending_wrap = false;
            }
            // DL — delete lines
            'M' => {
                let n = p1(0);
                for _ in 0..n {
                    if self.cursor_y < self.grid.len() {
                        self.grid.remove(self.cursor_y);
                    }
                    let ins = (self.scroll_bottom + 1).min(self.grid.len());
                    self.grid.insert(ins, self.empty_row());
                }
                self.cursor_x = 0;
                self.pending_wrap = false;
            }

            // ── Character insertion / deletion ────────────────────────────────
            // DCH — delete characters
            'P' => {
                let empty = self.empty_cell();
                let n = p1(0);
                if self.cursor_y < self.grid.len() {
                    let row = &mut self.grid[self.cursor_y];
                    for _ in 0..n {
                        if self.cursor_x < row.len() {
                            row.remove(self.cursor_x);
                            row.push(empty);
                        }
                    }
                }
            }
            // ICH — insert blank characters
            '@' => {
                let empty = self.empty_cell();
                let n = p1(0);
                if self.cursor_y < self.grid.len() {
                    let row = &mut self.grid[self.cursor_y];
                    for _ in 0..n {
                        if row.len() >= self.cols {
                            row.pop();
                        }
                        row.insert(self.cursor_x, empty);
                    }
                }
            }
            // REP — repeat last printed character
            'b' => {
                let n = p1(0);
                // We'd need to store the last printed char; approximate with space.
                // For a proper impl, store `last_char: char` in Performer.
                let _ = n; // stub
            }

            // ── Cursor style (DECSCUSR) ───────────────────────────────────────
            'q' if intermediates.first() == Some(&b' ') => {
                match p(0) {
                    0 | 1 | 2 => self.cursor_style = CursorStyle::Block,
                    3 | 4 => self.cursor_style = CursorStyle::Underline,
                    5 | 6 => self.cursor_style = CursorStyle::Bar,
                    _ => {}
                }
                self.cursor_blinking = matches!(p(0), 0 | 1 | 3 | 5);
            }

            // ── Save / restore cursor (SCP / RCP) ─────────────────────────────
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),

            // ── Scroll region (DECSTBM) ───────────────────────────────────────
            'r' => {
                let top = p1(0).saturating_sub(1);
                let bot = if p(1) == 0 { self.rows - 1 } else { p(1) - 1 };
                if top < bot && bot < self.rows {
                    self.scroll_top = top;
                    self.scroll_bottom = bot;
                }
                // Cursor to home after setting scroll region
                self.cursor_x = 0;
                self.cursor_y = if self.origin_mode { self.scroll_top } else { 0 };
                self.pending_wrap = false;
            }

            // ── Device status report (DSR) ────────────────────────────────────
            // We can't write back to the pty from here without a channel; stub.
            'n' => {}

            // ── Device attributes (DA) ────────────────────────────────────────
            'c' => {}

            // ── Erase character to right (same as ECH) ───────────────────────
            // Already handled above as 'X'.

            // ── SGR — Select Graphic Rendition ───────────────────────────────
            'm' => {
                self.apply_sgr(params);
            }

            // ── Mode set/reset (SM/RM) — public modes ─────────────────────────
            'h' => {
                // e.g. CSI 4 h — insert mode (stub)
            }
            'l' => {
                // e.g. CSI 4 l — replace mode (stub)
            }

            _ => {}
        }
    }

    // ── OSC sequences ─────────────────────────────────────────────────────────

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // params[0] is the command number as ASCII bytes, params[1..] are args.
        let cmd = params
            .first()
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(u32::MAX);

        let arg = |i: usize| -> &[u8] { params.get(i).copied().unwrap_or(b"") };

        match cmd {
            // OSC 0 / 1 / 2 — set icon name / title (callers can poll a title field)
            0 | 1 | 2 => {
                // To expose the title add a `pub title: String` field and set it here.
                let _ = arg(1); // UTF-8 title bytes
            }

            // OSC 4 — set color palette entry
            4 => {
                let mut i = 1;
                while i + 1 < params.len() {
                    let idx_bytes = arg(i);
                    let spec = arg(i + 1);
                    if let (Ok(idx_str), Ok(spec_str)) =
                        (std::str::from_utf8(idx_bytes), std::str::from_utf8(spec))
                    {
                        if let Ok(n) = idx_str.parse::<u8>() {
                            if let Some(color) = parse_color_spec(spec_str) {
                                self.palette_256[n as usize] = color;
                            }
                        }
                    }
                    i += 2;
                }
            }

            // OSC 8 — hyperlink  (OSC 8 ; params ; uri ST text OSC 8 ;; ST)
            // No rendering support needed here; ignore gracefully.
            8 => {}

            // OSC 10 — set / query default foreground color
            10 => {
                if let Ok(spec) = std::str::from_utf8(arg(1)) {
                    if spec != "?" {
                        if let Some(color) = parse_color_spec(spec) {
                            let old = self.default_fg;
                            self.default_fg = color;
                            if self.current_fg == old {
                                self.current_fg = color;
                            }
                        }
                    }
                }
            }
            // OSC 11 — set / query default background color
            11 => {
                if let Ok(spec) = std::str::from_utf8(arg(1)) {
                    if spec != "?" {
                        if let Some(color) = parse_color_spec(spec) {
                            let old = self.default_bg;
                            self.default_bg = color;
                            if self.current_bg == old {
                                self.current_bg = color;
                            }
                        }
                    }
                }
            }

            // OSC 52 — clipboard access (security: ignore set, can't query)
            52 => {}

            // OSC 133 — shell integration marks (A/B/C/D prompts)
            133 => {}

            _ => {}
        }
    }

    // ── DCS sequences ─────────────────────────────────────────────────────────

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        // DCS entry — e.g. DECRQSS, tmux passthrough
    }

    fn put(&mut self, _byte: u8) {
        // DCS data byte
    }

    fn unhook(&mut self) {
        // DCS end
    }
}

// ─── SGR helper ───────────────────────────────────────────────────────────────

impl Performer {
    fn apply_sgr(&mut self, params: &vte::Params) {
        let grouped_params: Vec<&[u16]> = params.iter().collect();
        if grouped_params.is_empty() {
            self.current_fg = self.default_fg;
            self.current_bg = self.default_bg;
            self.current_style = 0;
            return;
        }

        let mut i = 0;
        while i < grouped_params.len() {
            let group = grouped_params[i];
            let code = group.first().copied().unwrap_or(0);

            match code {
                0 => {
                    self.current_fg = self.default_fg;
                    self.current_bg = self.default_bg;
                    self.current_style = 0;
                }
                1 => self.current_style |= style::BOLD,
                2 => self.current_style |= style::DIM,
                3 => self.current_style |= style::ITALIC,
                4 => self.current_style |= style::UNDERLINE,
                5 | 6 => self.current_style |= style::BLINK,
                7 => self.current_style |= style::REVERSE,
                8 => self.current_style |= style::HIDDEN,
                9 => self.current_style |= style::STRIKETHROUGH,
                21 | 22 => self.current_style &= !(style::BOLD | style::DIM),
                23 => self.current_style &= !style::ITALIC,
                24 => self.current_style &= !style::UNDERLINE,
                25 => self.current_style &= !style::BLINK,
                27 => self.current_style &= !style::REVERSE,
                28 => self.current_style &= !style::HIDDEN,
                29 => self.current_style &= !style::STRIKETHROUGH,

                // Standard foreground (30–37, 39)
                30 => self.current_fg = self.palette_256[0],
                31 => self.current_fg = self.palette_256[1],
                32 => self.current_fg = self.palette_256[2],
                33 => self.current_fg = self.palette_256[3],
                34 => self.current_fg = self.palette_256[4],
                35 => self.current_fg = self.palette_256[5],
                36 => self.current_fg = self.palette_256[6],
                37 => self.current_fg = self.palette_256[7],
                39 => self.current_fg = self.default_fg,

                // Extended foreground: 38;5;n  or  38;2;r;g;b
                38 => {
                    // Colon-form SGR can arrive as one grouped parameter,
                    // e.g. 38:2::R:G:B.
                    if group.len() > 1 {
                        if let Some(color) =
                            parse_sgr_extended_color_group(group, &self.palette_256)
                        {
                            self.current_fg = color;
                        }
                    } else {
                        match grouped_params
                            .get(i + 1)
                            .and_then(|g| g.first())
                            .copied()
                            .unwrap_or(0)
                        {
                            5 if i + 2 < grouped_params.len() => {
                                let n = grouped_params[i + 2].first().copied().unwrap_or(0);
                                self.current_fg = self.palette_256[clamp_u16_to_u8(n) as usize];
                                i += 2;
                            }
                            2 if i + 4 < grouped_params.len() => {
                                let r = grouped_params[i + 2].first().copied().unwrap_or(0);
                                let g = grouped_params[i + 3].first().copied().unwrap_or(0);
                                let b = grouped_params[i + 4].first().copied().unwrap_or(0);
                                self.current_fg = Color::rgb(
                                    clamp_u16_to_u8(r),
                                    clamp_u16_to_u8(g),
                                    clamp_u16_to_u8(b),
                                );
                                i += 4;
                            }
                            _ => {}
                        }
                    }
                }

                // Standard background (40–47, 49)
                40 => self.current_bg = self.palette_256[0],
                41 => self.current_bg = self.palette_256[1],
                42 => self.current_bg = self.palette_256[2],
                43 => self.current_bg = self.palette_256[3],
                44 => self.current_bg = self.palette_256[4],
                45 => self.current_bg = self.palette_256[5],
                46 => self.current_bg = self.palette_256[6],
                47 => self.current_bg = self.palette_256[7],
                49 => self.current_bg = self.default_bg,

                // Extended background: 48;5;n  or  48;2;r;g;b
                48 => {
                    if group.len() > 1 {
                        if let Some(color) =
                            parse_sgr_extended_color_group(group, &self.palette_256)
                        {
                            self.current_bg = color;
                        }
                    } else {
                        match grouped_params
                            .get(i + 1)
                            .and_then(|g| g.first())
                            .copied()
                            .unwrap_or(0)
                        {
                            5 if i + 2 < grouped_params.len() => {
                                let n = grouped_params[i + 2].first().copied().unwrap_or(0);
                                self.current_bg = self.palette_256[clamp_u16_to_u8(n) as usize];
                                i += 2;
                            }
                            2 if i + 4 < grouped_params.len() => {
                                let r = grouped_params[i + 2].first().copied().unwrap_or(0);
                                let g = grouped_params[i + 3].first().copied().unwrap_or(0);
                                let b = grouped_params[i + 4].first().copied().unwrap_or(0);
                                self.current_bg = Color::rgb(
                                    clamp_u16_to_u8(r),
                                    clamp_u16_to_u8(g),
                                    clamp_u16_to_u8(b),
                                );
                                i += 4;
                            }
                            _ => {}
                        }
                    }
                }

                // Bright foreground (90–97)
                90 => self.current_fg = self.palette_256[8],
                91 => self.current_fg = self.palette_256[9],
                92 => self.current_fg = self.palette_256[10],
                93 => self.current_fg = self.palette_256[11],
                94 => self.current_fg = self.palette_256[12],
                95 => self.current_fg = self.palette_256[13],
                96 => self.current_fg = self.palette_256[14],
                97 => self.current_fg = self.palette_256[15],

                // Bright background (100–107)
                100 => self.current_bg = self.palette_256[8],
                101 => self.current_bg = self.palette_256[9],
                102 => self.current_bg = self.palette_256[10],
                103 => self.current_bg = self.palette_256[11],
                104 => self.current_bg = self.palette_256[12],
                105 => self.current_bg = self.palette_256[13],
                106 => self.current_bg = self.palette_256[14],
                107 => self.current_bg = self.palette_256[15],

                _ => {}
            }
            i += 1;
        }
    }
}

fn clamp_u16_to_u8(v: u16) -> u8 {
    v.min(u8::MAX as u16) as u8
}

fn parse_sgr_extended_color_group(group: &[u16], palette_256: &[Color; 256]) -> Option<Color> {
    match group.get(1).copied() {
        // 38:5:n or 48:5:n
        Some(5) => group
            .get(2)
            .copied()
            .map(clamp_u16_to_u8)
            .map(|idx| palette_256[idx as usize]),
        // 38:2:R:G:B, 38:2::R:G:B, or 38:2:color_space:R:G:B
        Some(2) => {
            if group.len() >= 6 {
                Some(Color::rgb(
                    clamp_u16_to_u8(group[3]),
                    clamp_u16_to_u8(group[4]),
                    clamp_u16_to_u8(group[5]),
                ))
            } else if group.len() >= 5 {
                Some(Color::rgb(
                    clamp_u16_to_u8(group[2]),
                    clamp_u16_to_u8(group[3]),
                    clamp_u16_to_u8(group[4]),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

// ─── Color helpers ────────────────────────────────────────────────────────────

fn default_palette_256() -> [Color; 256] {
    let mut palette = [Color::rgb(0, 0, 0); 256];
    for i in 0..=u8::MAX {
        palette[i as usize] = xterm_color_from_256(i);
    }
    palette
}

fn xterm_color_from_256(n: u8) -> Color {
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

/// Parse an X11 / xterm color specification such as `#rrggbb` or `rgb:rr/gg/bb`.
fn parse_color_spec(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        // #rgb or #rrggbb
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            _ => None,
        }
    } else if let Some(rest) = s.strip_prefix("rgb:") {
        // rgb:rr/gg/bb  (each component 1–4 hex digits; we take the top 2)
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() != 3 {
            return None;
        }
        let component = |p: &str| -> Option<u8> {
            let clamped = &p[..p.len().min(2)];
            u8::from_str_radix(clamped, 16).ok()
        };
        Some(Color::rgb(
            component(parts[0])?,
            component(parts[1])?,
            component(parts[2])?,
        ))
    } else {
        None
    }
}

fn charset_from_designator(designator: u8) -> Charset {
    match designator {
        b'0' => Charset::DecSpecialGraphics,
        _ => Charset::Ascii,
    }
}

fn map_dec_special_graphics(c: char) -> char {
    match c {
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'q' => '─',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}
// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> Terminal {
        Terminal::new()
    }

    #[test]
    fn test_bold_flag() {
        let mut t = term();
        t.process(b"\x1b[1mA");
        assert!(t.performer.grid[0][0].style & style::BOLD != 0);
    }

    #[test]
    fn test_reset_bold() {
        let mut t = term();
        t.process(b"\x1b[1mA\x1b[22mB");
        assert!(t.performer.grid[0][0].style & style::BOLD != 0);
        assert!(t.performer.grid[0][1].style & style::BOLD == 0);
    }

    #[test]
    fn test_color() {
        let mut t = term();
        t.process(b"\x1b[31mR\x1b[32mG");
        assert_eq!(t.performer.grid[0][0].fg, Color::rgb(205, 0, 0));
        assert_eq!(t.performer.grid[0][1].fg, Color::rgb(0, 205, 0));
    }

    #[test]
    fn test_cursor_movement() {
        let mut t = term();
        t.process(b"\x1b[5;10H"); // row 5, col 10 (1-based)
        assert_eq!(t.performer.cursor_y, 4);
        assert_eq!(t.performer.cursor_x, 9);
    }

    #[test]
    fn test_auto_wrap() {
        let mut t = term();
        // fill exactly 80 chars then one more
        let line: Vec<u8> = b"A".repeat(COLS);
        t.process(&line);
        assert!(t.performer.pending_wrap);
        t.process(b"B");
        assert_eq!(t.performer.cursor_y, 1);
        assert_eq!(t.performer.cursor_x, 1);
        assert_eq!(t.performer.grid[1][0].c, 'B');
    }

    #[test]
    fn test_scroll_up_on_newline() {
        let mut t = term();
        // Move to last row and emit a newline
        t.process(b"\x1b[24;1H\n");
        assert_eq!(t.performer.cursor_y, ROWS - 1);
        assert_eq!(t.performer.scrollback.len(), 1);
    }

    #[test]
    fn test_save_restore_cursor() {
        let mut t = term();
        t.process(b"\x1b[3;5H"); // move to 3,5
        t.process(b"\x1b7"); // DECSC save
        t.process(b"\x1b[1;1H"); // move elsewhere
        t.process(b"\x1b8"); // DECRC restore
        assert_eq!(t.performer.cursor_y, 2);
        assert_eq!(t.performer.cursor_x, 4);
    }

    #[test]
    fn test_alt_screen() {
        let mut t = term();
        t.process(b"hello");
        t.process(b"\x1b[?1049h"); // enter alt
        assert!(t.performer.in_alt_screen);
        assert_eq!(t.performer.grid[0][0].c, ' '); // blank alt screen
        t.process(b"\x1b[?1049l"); // exit alt
        assert!(!t.performer.in_alt_screen);
        assert_eq!(t.performer.grid[0][0].c, 'h'); // original content back
    }

    #[test]
    fn test_cursor_visibility() {
        let mut t = term();
        t.process(b"\x1b[?25l");
        assert!(!t.performer.cursor_visible);
        t.process(b"\x1b[?25h");
        assert!(t.performer.cursor_visible);
    }

    #[test]
    fn test_scroll_region() {
        let mut t = term();
        t.process(b"\x1b[5;10r"); // set scroll region rows 5–10
        assert_eq!(t.performer.scroll_top, 4);
        assert_eq!(t.performer.scroll_bottom, 9);
        // Cursor should home
        assert_eq!(t.performer.cursor_y, 0);
        assert_eq!(t.performer.cursor_x, 0);
    }

    #[test]
    fn test_erase_line() {
        let mut t = term();
        t.process(b"Hello\x1b[2K"); // write then erase whole line
        for x in 0..COLS {
            assert_eq!(t.performer.grid[0][x].c, ' ');
        }
    }

    #[test]
    fn test_erase_display_full_screen() {
        let mut t = term();
        t.process(b"Hello");
        t.process(b"\x1b[2J");
        for row in 0..ROWS {
            for col in 0..COLS {
                assert_eq!(t.performer.grid[row][col].c, ' ');
            }
        }
        assert_eq!(t.performer.cursor_x, 0);
        assert_eq!(t.performer.cursor_y, 0);
    }

    #[test]
    fn test_insert_line_once() {
        let mut t = term();
        t.process(b"\x1b[1;1HA");
        t.process(b"\x1b[2;1HB");
        t.process(b"\x1b[3;1HC");

        t.process(b"\x1b[2;1H\x1b[1L");

        assert_eq!(t.performer.grid[0][0].c, 'A');
        assert_eq!(t.performer.grid[1][0].c, ' ');
        assert_eq!(t.performer.grid[2][0].c, 'B');
        assert_eq!(t.performer.grid[3][0].c, 'C');
    }

    #[test]
    fn test_256_color() {
        let mut t = term();
        t.process(b"\x1b[38;5;196m"); // bright red index 196
        // 196 = 16 + 36*5 + 6*0 + 0  → r=5,g=0,b=0 → rgb(255,0,0)
        assert_eq!(t.performer.current_fg, Color::rgb(255, 0, 0));
    }

    #[test]
    fn test_truecolor() {
        let mut t = term();
        t.process(b"\x1b[38;2;10;20;30m");
        assert_eq!(t.performer.current_fg, Color::rgb(10, 20, 30));
    }

    #[test]
    fn test_truecolor_colon_form() {
        let mut t = term();
        t.process(b"\x1b[38:2::10:20:30m");
        assert_eq!(t.performer.current_fg, Color::rgb(10, 20, 30));
    }

    #[test]
    fn test_reverse_index() {
        let mut t = term();
        t.process(b"\x1b[3;1H"); // row 3
        t.process(b"\x1bM"); // RI: should move up without scroll
        assert_eq!(t.performer.cursor_y, 1);
    }

    #[test]
    fn test_ris_reset() {
        let mut t = term();
        t.process(b"\x1b[1mA\x1bc"); // bold A then RIS
        assert_eq!(t.performer.current_style, 0);
        assert_eq!(t.performer.cursor_x, 0);
        assert_eq!(t.performer.cursor_y, 0);
    }

    #[test]
    fn test_dec_special_graphics_shift() {
        let mut t = term();
        // Designate G1 as DEC special graphics, switch to G1 (SO), then back to G0 (SI).
        t.process(b"\x1b)0\x0eqx\x0fqq");
        assert_eq!(t.performer.grid[0][0].c, '─');
        assert_eq!(t.performer.grid[0][1].c, '│');
        assert_eq!(t.performer.grid[0][2].c, 'q');
        assert_eq!(t.performer.grid[0][3].c, 'q');
    }
}
