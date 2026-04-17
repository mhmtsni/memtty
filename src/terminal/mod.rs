use vte::Parser;

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

mod cell;
mod charset;
mod colors;
pub mod performer;
mod sgr;

pub use cell::Cell;
pub use performer::{CursorStyle, Performer};

#[derive(Default)]
pub struct Terminal {
    pub parser: Parser,
    pub performer: Performer,
}

impl Terminal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.parser.advance(&mut self.performer, bytes);
        self.performer.drain_pty_replies()
    }

    /// Visible rows for rendering, honoring scroll offset (positive = scrolled up).
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

#[cfg(test)]
mod tests;
