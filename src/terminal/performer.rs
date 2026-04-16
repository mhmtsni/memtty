use std::collections::VecDeque;

use glyphon::Color;
use vte::Perform;

use super::{
    cell::Cell,
    charset::{Charset, charset_from_designator, map_dec_special_graphics},
    colors::{DEFAULT_BG, DEFAULT_FG, MAX_SCROLLBACK, default_palette_256, parse_color_spec},
};

const DEFAULT_ROWS: usize = 24;
const DEFAULT_COLS: usize = 80;

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

#[derive(Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
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
    pub(super) scroll_top: usize,
    pub(super) scroll_bottom: usize,

    // ── SGR state ────────────────────────────────────────────────────────────
    pub(super) default_fg: Color,
    pub(super) default_bg: Color,
    pub(super) palette_256: [Color; 256],
    pub current_fg: Color,
    pub current_bg: Color,
    pub current_style: u8,

    // ── modes ─────────────────────────────────────────────────────────────────
    /// Auto-wrap mode (default on).
    pub(super) auto_wrap: bool,
    /// DEC origin mode: cursor movement is relative to scroll region.
    pub(super) origin_mode: bool,
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
    pub(super) pending_wrap: bool,
    focus_enable: bool,

    // Bytes that should be written back to the PTY (DA/DSR replies, etc.).
    pty_replies: Vec<Vec<u8>>,

    pub title: String,
}

impl Default for Performer {
    fn default() -> Self {
        let rows = DEFAULT_ROWS;
        let cols = DEFAULT_COLS;
        let palette_256 = default_palette_256();
        let default_fg = DEFAULT_FG;
        let default_bg = DEFAULT_BG;
        let default_cell = Cell {
            c: ' ',
            fg: default_fg,
            is_selected: false,
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
            title: String::new(),
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
            focus_enable: false,
            pty_replies: Vec::new(),
        }
    }
}

// ─── Performer helpers ────────────────────────────────────────────────────────

impl Performer {
    pub fn focus_reporting_enabled(&self) -> bool {
        self.focus_enable
    }

    pub fn drain_pty_replies(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pty_replies)
    }

    fn queue_pty_reply(&mut self, bytes: Vec<u8>) {
        self.pty_replies.push(bytes);
    }

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
            is_selected: false,
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
            is_selected: false,
            style: 0,
        }
    }

    fn empty_row(&self) -> Vec<Cell> {
        vec![self.empty_cell(); self.cols]
    }

    // ── scroll-region scroll ────────────────────────────────────────────────

    /// Scroll the scroll region up by `n` lines (content moves up, new blank
    /// lines appear at the bottom of the region).
    pub(super) fn scroll_up_region(&mut self, n: usize) {
        for _ in 0..n {
            // Fast path: full-screen scroll can use pop_front/push_back and append
            // into scrollback.
            let full_screen_region =
                self.scroll_top == 0 && self.scroll_bottom + 1 >= self.grid.len();

            if full_screen_region {
                if let Some(old_row) = self.grid.pop_front() {
                    self.scrollback.push_back(old_row);
                    if self.scrollback.len() > MAX_SCROLLBACK {
                        self.scrollback.pop_front();
                    }
                    self.grid.push_back(self.empty_row()); // push back at real bottom
                } else {
                    continue;
                }
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
    pub(super) fn scroll_down_region(&mut self, n: usize) {
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

    // ── cursor save / restore ───────────────────────────────────────────────

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

    // ── alt screen ──────────────────────────────────────────────────────────

    fn enter_alt_screen(&mut self) {
        if self.in_alt_screen {
            return;
        }
        // Save normal screen
        let blank = Cell {
            c: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            is_selected: false,
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

    // ── DEC private mode set/reset ──────────────────────────────────────────

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
            1004 => {
                self.focus_enable = enable;
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
    // ── printable character ─────────────────────────────────────────────────
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
                is_selected: false,
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

    // ── C0 / C1 control codes ───────────────────────────────────────────────

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

    // ── ESC sequences (not CSI, not OSC) ───────────────────────────────────

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

    // ── CSI sequences ───────────────────────────────────────────────────────

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
                // DEC DSR replies.
                'n' => {
                    for &code in &params_vec {
                        match code {
                            5 => self.queue_pty_reply(b"\x1b[?0n".to_vec()),
                            6 => {
                                let row = self.cursor_y + 1;
                                let col = self.cursor_x + 1;
                                self.queue_pty_reply(
                                    format!("\x1b[?{};{}R", row, col).into_bytes(),
                                );
                            }
                            _ => {}
                        }
                    }
                }
                // DECTCEM aliases handled inside set_dec_mode (mode 25)
                _ => {}
            }
            return;
        }

        // CSI > — secondary DA / xterm version
        if intermediates.first() == Some(&b'>') {
            if action == 'c' {
                self.queue_pty_reply(b"\x1b[>0;0;0c".to_vec());
            }
            return;
        }

        match action {
            // ── Cursor movement ─────────────────────────────────────────────
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

            // ── Erase ───────────────────────────────────────────────────────
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

            // ── Scroll ──────────────────────────────────────────────────────
            // SU — scroll up
            'S' => self.scroll_up(p1(0)),
            // SD — scroll down
            'T' => self.scroll_down(p1(0)),

            // ── Line insertion / deletion ───────────────────────────────────
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

            // ── Character insertion / deletion ─────────────────────────────
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

            // ── Cursor style (DECSCUSR) ─────────────────────────────────────
            'q' if intermediates.first() == Some(&b' ') => {
                match p(0) {
                    0 | 1 | 2 => self.cursor_style = CursorStyle::Block,
                    3 | 4 => self.cursor_style = CursorStyle::Underline,
                    5 | 6 => self.cursor_style = CursorStyle::Bar,
                    _ => {}
                }
                self.cursor_blinking = matches!(p(0), 0 | 1 | 3 | 5);
            }

            // ── Save / restore cursor (SCP / RCP) ───────────────────────────
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),

            // ── Scroll region (DECSTBM) ─────────────────────────────────────
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

            // ── Device status report (DSR) ──────────────────────────────────
            'n' => {
                for &code in &params_vec {
                    match code {
                        5 => self.queue_pty_reply(b"\x1b[0n".to_vec()),
                        6 => {
                            let row = self.cursor_y + 1;
                            let col = self.cursor_x + 1;
                            self.queue_pty_reply(format!("\x1b[{};{}R", row, col).into_bytes());
                        }
                        _ => {}
                    }
                }
            }

            // ── Device attributes (DA) ──────────────────────────────────────
            'c' => {
                self.queue_pty_reply(b"\x1b[?1;2c".to_vec());
            }

            // ── SGR — Select Graphic Rendition ─────────────────────────────
            'm' => {
                self.apply_sgr(params);
            }

            // ── Mode set/reset (SM/RM) — public modes ──────────────────────
            'h' => {
                // e.g. CSI 4 h — insert mode (stub)
            }
            'l' => {
                // e.g. CSI 4 l — replace mode (stub)
            }

            _ => {}
        }
    }

    // ── OSC sequences ───────────────────────────────────────────────────────

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
                // vte splits OSC params on ';', so a title that contains ';'
                // would be spread across params[1..]. Re-join with ';'.
                if params.len() <= 1 {
                    self.title.clear();
                } else {
                    let mut title_bytes: Vec<u8> = Vec::new();
                    for (i, part) in params.iter().enumerate().skip(1) {
                        if i > 1 {
                            title_bytes.push(b';');
                        }
                        title_bytes.extend_from_slice(part);
                    }
                    self.title = String::from_utf8_lossy(&title_bytes).to_string();
                }
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

    // ── DCS sequences ───────────────────────────────────────────────────────

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
