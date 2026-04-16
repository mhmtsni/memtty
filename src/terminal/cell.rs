use glyphon::Color;

use super::colors::{DEFAULT_BG, DEFAULT_FG};

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub is_selected: bool,
    pub style: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: DEFAULT_FG,
            is_selected: false,
            bg: DEFAULT_BG,
            style: 0,
        }
    }
}
