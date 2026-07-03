use unicode_width::UnicodeWidthChar;

use super::{Cell, Performer};

impl Performer {
    pub(super) fn print_char(&mut self, c: char) {
        let c = self.translate_char(c);

        const ZWJ: char = '\u{200d}';
        const VS15: char = '\u{fe0e}';
        const VS16: char = '\u{fe0f}';

        let is_emoji_modifier = ('\u{1f3fb}'..='\u{1f3ff}').contains(&c);
        let is_grapheme_extend = c == ZWJ
            || c == VS15
            || c == VS16
            || is_emoji_modifier
            || UnicodeWidthChar::width(c).unwrap_or(0) == 0;

        if (self.join_next_to_last_cell || is_grapheme_extend)
            && self.append_to_last_cell_grapheme(c)
        {
            self.join_next_to_last_cell = c == ZWJ;
            return;
        }

        let width = UnicodeWidthChar::width(c).unwrap_or(0);
        if width == 0 {
            return;
        }

        if self.pending_wrap && self.auto_wrap {
            self.cursor_x = 0;
            self.cursor_y += 1;
            if self.cursor_y > self.scroll_bottom {
                self.scroll_up_region(1);
                self.cursor_y = self.scroll_bottom;
            }
            self.pending_wrap = false;
        }

        if self.cursor_y >= self.grid.len() {
            return;
        }

        let row_len = self.grid[self.cursor_y].len();
        if self.cursor_x >= row_len {
            return;
        }

        if self.insert_mode {
            let shift = width.min(row_len.saturating_sub(self.cursor_x));
            if shift > 0 {
                let empty = self.empty_cell();
                let row = &mut self.grid[self.cursor_y];
                for _ in 0..shift {
                    row.pop();
                    row.insert(self.cursor_x, empty.clone());
                }
            }
        }

        if width == 2 && self.cursor_x + 1 >= row_len {
            if self.auto_wrap {
                self.pending_wrap = true;
            }
            return;
        }

        self.grid[self.cursor_y][self.cursor_x] = Cell {
            c,
            text: c.to_string().into(),
            wide_continuation: false,
            hyperlink: self.current_hyperlink.clone(),
            is_link_hovered: false,
            fg: self.current_fg,
            bg: self.current_bg,
            is_selected: false,
            style: self.current_style,
        };
        self.last_cell_pos = Some((self.cursor_x, self.cursor_y));
        self.join_next_to_last_cell = false;

        if width == 2 {
            self.grid[self.cursor_y][self.cursor_x + 1] = Cell {
                c: ' ',
                text: smol_str::SmolStr::default(),
                wide_continuation: true,
                hyperlink: self.current_hyperlink.clone(),
                is_link_hovered: false,
                fg: self.current_fg,
                bg: self.current_bg,
                is_selected: false,
                style: self.current_style,
            };
            self.last_printed = Some(c);

            if self.cursor_x + 2 >= row_len {
                self.pending_wrap = true;
            } else {
                self.cursor_x += 2;
            }
            return;
        }

        self.last_printed = Some(c);

        if self.cursor_x + 1 >= row_len {
            self.pending_wrap = true;
        } else {
            self.cursor_x += 1;
        }
    }
}
