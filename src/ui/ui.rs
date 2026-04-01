use std::{fmt::Debug, sync::Arc};

use arboard::Clipboard;
use tokio::sync::mpsc::UnboundedSender;
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseScrollDelta},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Fullscreen, Window},
};

use crate::{
    pty::PtyInput,
    terminal::{CursorStyle, Terminal},
    ui::renderer::{CursorRenderInfo, CursorRenderStyle, Renderer},
};

#[derive(Clone, Debug)]
pub enum Message {
    PtyDataReceived(Vec<u8>),
    PtyExited,
}

use std::time::{Duration, Instant};

const CURSOR_COLOR: glyphon::Color = glyphon::Color::rgb(255, 255, 255);

pub struct MyApp {
    window: Arc<Window>,
    tx: Option<UnboundedSender<PtyInput>>,
    pub terminal: Terminal,
    pub scroll_offset: i32,
    full_screen: bool,
    modifiers: ModifiersState,
    pub renderer: Renderer,
    cursor_blink_on: bool,
    last_blink: Instant,
    pub has_focus: bool,
}

impl MyApp {
    fn cursor_blink_active(&self) -> bool {
        self.terminal.performer.cursor_blinking || !self.has_focus
    }

    pub fn new(
        window: Arc<Window>,
        tx_to_pty: UnboundedSender<PtyInput>,
        renderer: Renderer,
    ) -> Self {
        let mut app = Self {
            full_screen: false,
            tx: Some(tx_to_pty),
            terminal: Terminal::new(),
            scroll_offset: 0,
            window,
            modifiers: ModifiersState::empty(),
            renderer,
            cursor_blink_on: true,
            last_blink: std::time::Instant::now(),
            has_focus: true,
        };

        app.sync_renderer_from_terminal(true);
        app
    }

    pub fn sync_renderer_from_terminal(&mut self, content_changed: bool) {
        let visible_rows = self.renderer.visible_row_capacity();
        let rows = self.terminal.visible_rows(self.scroll_offset, visible_rows);

        let cursor = self.visible_cursor_info(visible_rows, rows.len());
        self.renderer.set_cells(&rows, cursor, content_changed);
    }

    fn visible_cursor_info(
        &self,
        requested_visible_rows: usize,
        actual_visible_rows: usize,
    ) -> Option<CursorRenderInfo> {
        if !self.terminal.performer.cursor_visible || self.scroll_offset != 0 {
            return None;
        }

        let scrollback_len = self.terminal.performer.scrollback.len();
        let grid_len = self.terminal.performer.grid.len();
        let total_rows = scrollback_len + grid_len;
        if total_rows == 0 {
            return None;
        }

        let offset = self.scroll_offset.max(0) as usize;
        let end = total_rows.saturating_sub(offset);
        let start = end.saturating_sub(requested_visible_rows);

        let cursor_abs_row = scrollback_len + self.terminal.performer.cursor_y;
        if cursor_abs_row < start || cursor_abs_row >= end {
            return None;
        }

        let cursor_row = cursor_abs_row - start;
        if cursor_row >= actual_visible_rows {
            return None;
        }

        let cursor_style = match self.terminal.performer.cursor_style {
            CursorStyle::Block => CursorRenderStyle::Block,
            CursorStyle::Underline => CursorRenderStyle::Underline,
            CursorStyle::Bar => CursorRenderStyle::Bar,
        };

        Some(CursorRenderInfo {
            col: self.terminal.performer.cursor_x,
            row: cursor_row,
            style: cursor_style,
            color: CURSOR_COLOR,
            blink_on: !self.cursor_blink_active() || self.cursor_blink_on,
        })
    }

    fn send_to_pty(&mut self, data: PtyInput) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(data);
        }
    }

    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    pub fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let scroll_amount = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as i32,
            MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
        };

        if scroll_amount == 0 {
            return;
        }

        let max_offset = self.terminal.performer.scrollback.len() as i32;
        self.scroll_offset = (self.scroll_offset + scroll_amount).max(0).min(max_offset);
        self.terminal.performer.cursor_visible = self.scroll_offset == 0;
        self.sync_renderer_from_terminal(true);
    }

    pub fn handle_resize(&mut self, size: PhysicalSize<u32>) {
        let width = size.width as f32;
        let height = size.height as f32;
        let (cell_width, line_height) = self.renderer.cell_size();

        let new_cols = (width / cell_width).floor().max(10.0) as u16;
        let new_rows = (height / line_height).floor().max(5.0) as u16;

        self.terminal
            .performer
            .resize(new_cols as usize, new_rows as usize);

        self.send_to_pty(PtyInput::Resize {
            cols: new_cols,
            rows: new_rows,
        });

        self.renderer.resize(size.width, size.height);

        let max_offset = self.terminal.performer.scrollback.len() as i32;
        self.scroll_offset = self.scroll_offset.min(max_offset).max(0);
        self.sync_renderer_from_terminal(true);
    }

    pub fn handle_key_event(&mut self, event: KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }

        if self.modifiers.super_key()
            && matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"))
        {
            self.handle_paste();
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Enter)) && self.modifiers.super_key() {
            self.full_screen = !self.full_screen;
            let mode = if self.full_screen {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            };
            self.window.set_fullscreen(mode);
            return;
        }

        if self.modifiers.super_key() {
            if let Key::Character(c) = &event.logical_key {
                match c.to_lowercase().as_str() {
                    "+" | "=" => {
                        let new_size = self.renderer.font_size + 2.0;
                        self.renderer.set_font_size(new_size);
                        self.refit_terminal_to_renderer();
                        return;
                    }
                    "-" => {
                        let new_size = self.renderer.font_size - 2.0;
                        self.renderer.set_font_size(new_size);
                        self.refit_terminal_to_renderer();
                        return;
                    }
                    "0" => {
                        self.renderer.reset_font_size();
                        self.refit_terminal_to_renderer();
                        return;
                    }
                    _ => return,
                }
            }
        }

        if let Some(bytes) = self.map_key_to_bytes(&event) {
            self.send_to_pty(PtyInput::Data(bytes));
            self.reset_scrollback_view();
            return;
        }

        if let Some(text) = event.text.as_ref() {
            if !text.is_empty() {
                self.send_to_pty(PtyInput::Data(text.as_bytes().to_vec()));
                self.reset_scrollback_view();
            }
        }
    }

    fn handle_paste(&mut self) {
        let Ok(mut clipboard) = Clipboard::new() else {
            return;
        };

        let Ok(text) = clipboard.get_text() else {
            return;
        };

        if text.is_empty() {
            return;
        }

        let normalized = text.replace("\r\n", "\n").replace('\n', "\r");

        let mut data = Vec::with_capacity(normalized.len() + 12);
        data.extend_from_slice(b"\x1b[200~");
        data.extend_from_slice(normalized.as_bytes());
        data.extend_from_slice(b"\x1b[201~");

        self.send_to_pty(PtyInput::Data(data));
        self.reset_scrollback_view();
    }

    fn map_key_to_bytes(&mut self, event: &KeyEvent) -> Option<Vec<u8>> {
        let key = &event.logical_key;

        match key {
            Key::Named(NamedKey::ArrowUp) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5A".to_vec())
            }
            Key::Named(NamedKey::ArrowDown) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5B".to_vec())
            }
            Key::Named(NamedKey::ArrowRight) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5C".to_vec())
            }
            Key::Named(NamedKey::ArrowLeft) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5D".to_vec())
            }
            Key::Named(NamedKey::Backspace) if self.modifiers.alt_key() => {
                Some(b"\x1b\x7f".to_vec())
            }
            Key::Character(c) if self.modifiers.control_key() => match c.to_lowercase().as_str() {
                "a" => Some(b"\x01".to_vec()),
                "b" => Some(b"\x02".to_vec()),
                "c" => Some(b"\x03".to_vec()),
                "d" => Some(b"\x04".to_vec()),
                "e" => Some(b"\x05".to_vec()),
                "f" => Some(b"\x06".to_vec()),
                "g" => Some(b"\x07".to_vec()),
                "h" => Some(b"\x08".to_vec()),
                "i" => Some(b"\x09".to_vec()),
                "j" => Some(b"\x0a".to_vec()),
                "k" => Some(b"\x0b".to_vec()),
                "l" => Some(b"\x0c".to_vec()),
                "m" => Some(b"\x0d".to_vec()),
                "n" => Some(b"\x0e".to_vec()),
                "o" => Some(b"\x0f".to_vec()),
                "p" => Some(b"\x10".to_vec()),
                "q" => Some(b"\x11".to_vec()),
                "r" => Some(b"\x12".to_vec()),
                "s" => Some(b"\x13".to_vec()),
                "t" => Some(b"\x14".to_vec()),
                "u" => Some(b"\x15".to_vec()),
                "v" => Some(b"\x16".to_vec()),
                "w" => Some(b"\x17".to_vec()),
                "x" => Some(b"\x18".to_vec()),
                "y" => Some(b"\x19".to_vec()),
                "z" => Some(b"\x1a".to_vec()),
                "[" => Some(b"\x1b".to_vec()),
                "\\" => Some(b"\x1c".to_vec()),
                "]" => Some(b"\x1d".to_vec()),
                _ => None,
            },

            Key::Named(NamedKey::Backspace) if self.modifiers.super_key() => Some(b"\x15".to_vec()),
            Key::Named(NamedKey::Tab) if self.modifiers.shift_key() => Some(b"\x1b[Z".to_vec()),
            Key::Named(NamedKey::Enter) => Some(b"\r".to_vec()),
            Key::Named(NamedKey::Backspace) => Some(b"\x7f".to_vec()),
            Key::Named(NamedKey::Escape) => Some(b"\x1b".to_vec()),
            Key::Named(NamedKey::Tab) => Some(b"\t".to_vec()),
            Key::Named(NamedKey::Space) => Some(b" ".to_vec()),
            Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
            Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A".to_vec()),
            Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B".to_vec()),
            Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C".to_vec()),
            Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D".to_vec()),
            Key::Named(NamedKey::Home) => Some(b"\x1b[H".to_vec()),
            Key::Named(NamedKey::End) => Some(b"\x1b[F".to_vec()),
            Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
            Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
            Key::Named(NamedKey::F1) => Some(b"\x1bOP".to_vec()),
            Key::Named(NamedKey::F2) => Some(b"\x1bOQ".to_vec()),
            Key::Named(NamedKey::F3) => Some(b"\x1bOR".to_vec()),
            Key::Named(NamedKey::F4) => Some(b"\x1bOS".to_vec()),
            Key::Named(NamedKey::F5) => Some(b"\x1b[15~".to_vec()),
            Key::Named(NamedKey::F6) => Some(b"\x1b[17~".to_vec()),
            Key::Named(NamedKey::F7) => Some(b"\x1b[18~".to_vec()),
            Key::Named(NamedKey::F8) => Some(b"\x1b[19~".to_vec()),
            Key::Named(NamedKey::F9) => Some(b"\x1b[20~".to_vec()),
            Key::Named(NamedKey::F10) => Some(b"\x1b[21~".to_vec()),
            Key::Named(NamedKey::F11) => Some(b"\x1b[23~".to_vec()),
            Key::Named(NamedKey::F12) => Some(b"\x1b[24~".to_vec()),
            _ => None,
        }
    }

    fn refit_terminal_to_renderer(&mut self) {
        let (cell_width, line_height) = self.renderer.cell_size();
        let new_cols = (self.renderer.width as f32 / cell_width).floor().max(10.0) as u16;
        let new_rows = (self.renderer.height as f32 / line_height).floor().max(5.0) as u16;

        self.terminal
            .performer
            .resize(new_cols as usize, new_rows as usize);

        self.send_to_pty(PtyInput::Resize {
            cols: new_cols,
            rows: new_rows,
        });

        let max_offset = self.terminal.performer.scrollback.len() as i32;
        self.scroll_offset = self.scroll_offset.min(max_offset).max(0);
        self.sync_renderer_from_terminal(true);
    }

    fn reset_scrollback_view(&mut self) {
        self.scroll_offset = 0;
        self.terminal.performer.cursor_visible = true;
    }
    pub fn update_cursor_blink(&mut self) -> bool {
        let blink_interval = std::time::Duration::from_millis(500);
        let now = std::time::Instant::now();
        if self.cursor_blink_active()
            && self.terminal.performer.cursor_visible
            && now.duration_since(self.last_blink) >= blink_interval
        {
            self.cursor_blink_on = !self.cursor_blink_on;
            self.last_blink = now;
            self.sync_renderer_from_terminal(false);
            return true;
        }
        false
    }

    pub fn next_blink_deadline(&self) -> Option<Instant> {
        if self.cursor_blink_active() && self.terminal.performer.cursor_visible {
            Some(self.last_blink + Duration::from_millis(500))
        } else {
            None
        }
    }
    pub fn update_has_focus(&mut self, has_focus: bool) {
        if self.has_focus == has_focus {
            return;
        }

        let focus_reporting_enabled = self.terminal.performer.focus_reporting_enabled();
        self.has_focus = has_focus;

        if focus_reporting_enabled {
            let sequence = if has_focus {
                b"\x1b[I".to_vec()
            } else {
                b"\x1b[O".to_vec()
            };
            self.send_to_pty(PtyInput::Data(sequence));
        }

        self.last_blink = std::time::Instant::now();
        self.sync_renderer_from_terminal(false);
    }
}
