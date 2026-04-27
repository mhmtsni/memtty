use super::*;

impl MyApp {
    pub(super) fn copy_selection_to_clipboard(&mut self) -> bool {
        let Some(text) = self.selected_text() else {
            return false;
        };

        if text.is_empty() {
            return false;
        }

        let Ok(mut clipboard) = Clipboard::new() else {
            return false;
        };

        clipboard.set_text(text).is_ok()
    }

    pub(super) fn select_all(&mut self) -> bool {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return false;
        };

        let performer = &tab.terminal.performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return false;
        }

        let last_row = total_rows - 1;
        let scrollback_len = performer.scrollback.len();
        let last_row_len = if last_row < scrollback_len {
            performer.scrollback[last_row].len()
        } else {
            performer.grid[last_row - scrollback_len].len()
        };

        if last_row_len == 0 {
            return false;
        }

        self.selection_start = Some((0, 0));
        self.selection_end = Some((last_row_len - 1, last_row));
        self.selecting = false;
        true
    }

    fn selected_text(&self) -> Option<String> {
        let tab = self.tabs.get(self.active_tab)?;
        let performer = &tab.terminal.performer;

        let (Some((sx, sy)), Some((ex, ey))) = (self.selection_start, self.selection_end) else {
            return None;
        };

        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return None;
        }

        let mut a_row = sy.min(total_rows - 1);
        let mut a_col = sx;
        let mut b_row = ey.min(total_rows - 1);
        let mut b_col = ex;

        if (a_row, a_col) > (b_row, b_col) {
            std::mem::swap(&mut a_row, &mut b_row);
            std::mem::swap(&mut a_col, &mut b_col);
        }

        let scrollback_len = performer.scrollback.len();
        let mut lines = Vec::with_capacity(b_row.saturating_sub(a_row) + 1);

        for abs_row in a_row..=b_row {
            let row = if abs_row < scrollback_len {
                &performer.scrollback[abs_row]
            } else {
                &performer.grid[abs_row - scrollback_len]
            };

            if row.is_empty() {
                lines.push(String::new());
                continue;
            }

            let row_start = if abs_row == a_row {
                a_col.min(row.len() - 1)
            } else {
                0
            };
            let row_end = if abs_row == b_row {
                b_col.min(row.len() - 1)
            } else {
                row.len() - 1
            };

            if row_start > row_end {
                lines.push(String::new());
                continue;
            }

            let mut line: String = row[row_start..=row_end].iter().map(|cell| cell.c).collect();
            line.truncate(line.trim_end_matches(' ').len());
            lines.push(line);
        }

        Some(lines.join("\n"))
    }

    pub(super) fn handle_paste(&mut self) {
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

        let bracketed_paste_enabled = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.terminal.performer.bracketed_paste)
            .unwrap_or(false);

        let data = if bracketed_paste_enabled {
            let mut data = Vec::with_capacity(normalized.len() + 12);
            data.extend_from_slice(b"\x1b[200~");
            data.extend_from_slice(normalized.as_bytes());
            data.extend_from_slice(b"\x1b[201~");
            data
        } else {
            normalized.into_bytes()
        };

        self.send_to_pty(PtyInput::Data(data));
        self.reset_scrollback_view();
    }
}
