use glyphon::Color;
use smol_str::SmolStr;
use std::sync::Arc;

use super::colors::{DEFAULT_BG, DEFAULT_FG};

#[derive(Clone, Debug)]
pub struct Cell {
    pub c: char,
    pub text: SmolStr,
    // True when this cell is the trailing half of a double-width character.
    pub wide_continuation: bool,
    pub hyperlink: Option<Arc<str>>,
    pub is_link_hovered: bool,
    pub fg: Color,
    pub bg: Color,
    pub is_selected: bool,
    pub style: u8,
}

impl Cell {
    pub fn display_text(&self) -> &str {
        if self.wide_continuation {
            ""
        } else {
            &self.text
        }
    }

    pub fn is_blank(&self) -> bool {
        !self.wide_continuation && self.text == " "
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            text: SmolStr::new_inline(" "),
            wide_continuation: false,
            hyperlink: None,
            is_link_hovered: false,
            fg: DEFAULT_FG,
            is_selected: false,
            bg: DEFAULT_BG,
            style: 0,
        }
    }
}
