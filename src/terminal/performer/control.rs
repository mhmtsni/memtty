use super::{Performer, charset_from_designator};

impl Performer {
    pub(super) fn execute_control(&mut self, byte: u8) {
        match byte {
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
                let mut next = self.cols.saturating_sub(1);
                for i in (self.cursor_x + 1)..self.cols {
                    if self.tab_stops.get(i).copied().unwrap_or(false) {
                        next = i;
                        break;
                    }
                }
                self.cursor_x = next;
                self.pending_wrap = false;
            }
            0x08 => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
                self.pending_wrap = false;
            }
            0x7F | 0x07 => {}
            0x0E => self.use_g1_charset = true,
            0x0F => self.use_g1_charset = false,
            _ => {}
        }
    }

    pub(super) fn dispatch_escape(&mut self, intermediates: &[u8], byte: u8) {
        match (intermediates.first().copied(), byte) {
            (None, b'7') => self.save_cursor(),
            (None, b'8') => self.restore_cursor(),
            (None, b'D') => self.index_line(),
            (None, b'E') => {
                self.cursor_x = 0;
                self.index_line();
            }
            (None, b'M') => {
                if self.cursor_y == self.scroll_top {
                    self.scroll_down_region(1);
                } else {
                    self.cursor_y = self.cursor_y.saturating_sub(1);
                }
            }
            (None, b'H') => {
                if self.cursor_x < self.tab_stops.len() {
                    self.tab_stops[self.cursor_x] = true;
                }
            }
            (None, b'c') => {
                *self = Performer::default();
            }
            (Some(b'#'), b'8') => {
                for row in &mut self.grid {
                    for cell in row.iter_mut() {
                        cell.c = 'E';
                        cell.text.clear();
                        cell.text.push('E');
                        cell.wide_continuation = false;
                        cell.hyperlink = None;
                        cell.is_link_hovered = false;
                    }
                }
            }
            (Some(b'('), designator) => {
                self.g0_charset = charset_from_designator(designator);
            }
            (Some(b')'), designator) => {
                self.g1_charset = charset_from_designator(designator);
            }
            (Some(b'*'), _) | (Some(b'+'), _) => {}
            _ => {}
        }
    }

    fn index_line(&mut self) {
        if self.cursor_y == self.scroll_bottom {
            self.scroll_up_region(1);
        } else {
            self.cursor_y = (self.cursor_y + 1).min(self.rows - 1);
        }
    }
}
