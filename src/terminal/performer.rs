use std::collections::VecDeque;

use glyphon::Color;
use vte::Perform;

use super::{
    cell::Cell,
    charset::{Charset, charset_from_designator, map_dec_special_graphics},
    colors::{DEFAULT_BG, DEFAULT_FG, MAX_SCROLLBACK, default_palette_256, parse_color_spec},
};

mod control;
mod csi;
mod osc;
mod print;

const DEFAULT_ROWS: usize = 24;
const DEFAULT_COLS: usize = 80;

// Saved cursor state (DECSC/DECRC and CSI s/u).
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
    pub sgr_mouse: bool,

    // G0/G1 charset designations and active GL selection (SI/SO).
    g0_charset: Charset,
    g1_charset: Charset,
    use_g1_charset: bool,

    // pending wrap: next print will first advance to next line
    pub(super) pending_wrap: bool,
    focus_enable: bool,
    insert_mode: bool,
    tab_stops: Vec<bool>,
    last_printed: Option<char>,
    last_cell_pos: Option<(usize, usize)>,
    join_next_to_last_cell: bool,
    current_hyperlink: Option<String>,

    // Bytes that should be written back to the PTY (DA/DSR replies, etc.).
    pty_replies: Vec<Vec<u8>>,

    tmux_dcs_buffer: Option<Vec<u8>>,

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
            text: " ".to_string(),
            wide_continuation: false,
            hyperlink: None,
            is_link_hovered: false,
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
            sgr_mouse: false,
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
            insert_mode: false,
            tab_stops: (0..cols).map(|i| i % 8 == 0).collect(),
            last_printed: None,
            last_cell_pos: None,
            join_next_to_last_cell: false,
            current_hyperlink: None,
            pty_replies: Vec::new(),
            tmux_dcs_buffer: None,
        }
    }
}

// ─── Performer helpers ────────────────────────────────────────────────────────

impl Performer {
    pub fn focus_reporting_enabled(&self) -> bool {
        self.focus_enable
    }

    pub fn report_mouse(&mut self, x: usize, y: usize, button: u8, pressed: bool) {
        if self.mouse_mode == MouseMode::None {
            return;
        }

        let reply = if self.sgr_mouse {
            let suffix = if pressed { 'M' } else { 'm' };
            format!("\x1b[<{};{};{}{}", button, x + 1, y + 1, suffix)
        } else {
            // X10 encoding
            let b = 32 + button;
            let bx = 32 + (x + 1) as u8;
            let by = 32 + (y + 1) as u8;
            format!("\x1b[M{}{}{}", b as char, bx as char, by as char)
        };

        self.queue_pty_reply(reply.into_bytes());
    }

    pub fn drain_pty_replies(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pty_replies)
    }

    fn queue_pty_reply(&mut self, bytes: Vec<u8>) {
        self.pty_replies.push(bytes);
    }

    fn decode_tmux_dcs_passthrough(bytes: &[u8]) -> Option<Vec<u8>> {
        let inner = bytes.strip_prefix(b"mux;")?;
        let mut decoded = Vec::with_capacity(inner.len());
        let mut i = 0;

        while i < inner.len() {
            if inner[i] == 0x1b && inner.get(i + 1) == Some(&0x1b) {
                decoded.push(0x1b);
                i += 2;
            } else {
                decoded.push(inner[i]);
                i += 1;
            }
        }

        Some(decoded)
    }

    fn apply_tmux_dcs_passthrough(&mut self, bytes: &[u8]) {
        let Some(decoded) = Self::decode_tmux_dcs_passthrough(bytes) else {
            return;
        };

        let mut parser = vte::Parser::new();
        parser.advance(self, &decoded);
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        self.cols = new_cols;
        self.rows = new_rows;
        self.tab_stops.resize(new_cols, false);
        for (i, stop) in self.tab_stops.iter_mut().enumerate() {
            if i % 8 == 0 {
                *stop = true;
            }
        }
        self.normalize_grid_dimensions();
        self.scroll_top = 0;
        self.scroll_bottom = new_rows - 1;
        self.clamp_cursor();
    }

    fn normalize_grid_dimensions(&mut self) {
        let blank = Cell {
            c: ' ',
            text: " ".to_string(),
            wide_continuation: false,
            hyperlink: None,
            is_link_hovered: false,
            fg: self.current_fg,
            bg: self.current_bg,
            is_selected: false,
            style: 0,
        };
        self.grid.resize(self.rows, vec![blank.clone(); self.cols]);
        for row in &mut self.grid {
            row.resize(self.cols, blank.clone());
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
            text: " ".to_string(),
            wide_continuation: false,
            hyperlink: None,
            is_link_hovered: false,
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
            text: " ".to_string(),
            wide_continuation: false,
            hyperlink: None,
            is_link_hovered: false,
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
            1006 => {
                self.sgr_mouse = enable;
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

    fn append_to_last_cell_grapheme(&mut self, c: char) -> bool {
        let Some((x, y)) = self.last_cell_pos else {
            return false;
        };
        if y >= self.grid.len() || x >= self.grid[y].len() {
            return false;
        }

        let cell = &mut self.grid[y][x];
        if cell.wide_continuation {
            return false;
        }
        cell.text.push(c);
        true
    }
}

// ─── vte::Perform implementation ─────────────────────────────────────────────

impl Perform for Performer {
    // ── printable character ─────────────────────────────────────────────────
    fn print(&mut self, c: char) {
        self.print_char(c);
    }

    // ── C0 / C1 control codes ───────────────────────────────────────────────

    fn execute(&mut self, byte: u8) {
        self.execute_control(byte);
    }

    // ── ESC sequences (not CSI, not OSC) ───────────────────────────────────

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        self.dispatch_escape(intermediates, byte);
    }

    // ── CSI sequences ───────────────────────────────────────────────────────

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        self.dispatch_csi(params, intermediates, action);
    }

    // ── OSC sequences ───────────────────────────────────────────────────────

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        self.dispatch_osc(params);
    }

    // ── DCS sequences ───────────────────────────────────────────────────────

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        // Tmux wraps passthrough escapes as DCS "tmux;<escaped bytes>".
        self.tmux_dcs_buffer = (action == 't').then(Vec::new);
    }

    fn put(&mut self, byte: u8) {
        if let Some(buffer) = &mut self.tmux_dcs_buffer {
            buffer.push(byte);
        }
    }

    fn unhook(&mut self) {
        if let Some(buffer) = self.tmux_dcs_buffer.take() {
            self.apply_tmux_dcs_passthrough(&buffer);
        }
    }
}
