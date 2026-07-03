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
        let Some(tab) = self.session.tabs.get(self.session.active_tab) else {
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

        self.interaction.selection_start = Some((0, 0));
        self.interaction.selection_end = Some((last_row_len - 1, last_row));
        self.interaction.selecting = false;
        true
    }

    fn selected_text(&self) -> Option<String> {
        let tab = self.session.tabs.get(self.session.active_tab)?;
        let performer = &tab.terminal.performer;

        let total_rows = performer.scrollback.len() + performer.grid.len();
        let range = self.current_selection_range(total_rows)?;

        let scrollback_len = performer.scrollback.len();
        let mut lines = Vec::with_capacity(range.end.1.saturating_sub(range.start.1) + 1);

        for abs_row in range.start.1..=range.end.1 {
            let row = if abs_row < scrollback_len {
                &performer.scrollback[abs_row]
            } else {
                &performer.grid[abs_row - scrollback_len]
            };

            if row.is_empty() {
                lines.push(String::new());
                continue;
            }

            let row_start = if abs_row == range.start.1 {
                range.start.0.min(row.len() - 1)
            } else {
                0
            };
            let row_end = if abs_row == range.end.1 {
                range.end.0.min(row.len() - 1)
            } else {
                row.len() - 1
            };

            if row_start > row_end {
                lines.push(String::new());
                continue;
            }

            lines.push(selection_line_text(row, row_start, row_end));
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
            .session
            .tabs
            .get(self.session.active_tab)
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

fn selection_line_text(row: &[crate::terminal::Cell], row_start: usize, row_end: usize) -> String {
    let mut line: String = row[row_start..=row_end]
        .iter()
        .map(|cell| cell.display_text())
        .collect();
    line.truncate(line.trim_end_matches(' ').len());
    line
}

#[cfg(test)]
mod tests {
    use super::selection_line_text;
    use crate::terminal::Cell;

    #[test]
    fn selection_line_text_trims_trailing_spaces() {
        let row = [
            Cell {
                c: 'a',
                text: "a".to_string().into(),
                ..Default::default()
            },
            Cell {
                c: 'b',
                text: "b".to_string().into(),
                ..Default::default()
            },
            Cell::default(),
            Cell::default(),
        ];

        assert_eq!(selection_line_text(&row, 0, 3), "ab");
    }

    #[test]
    fn selection_line_text_ignores_wide_continuation_cells() {
        let row = [
            Cell {
                c: '中',
                text: "中".to_string().into(),
                wide_continuation: false,
                ..Default::default()
            },
            Cell {
                c: ' ',
                text: String::new().into(),
                wide_continuation: true,
                ..Default::default()
            },
            Cell {
                c: 'x',
                text: "x".to_string().into(),
                ..Default::default()
            },
        ];

        assert_eq!(selection_line_text(&row, 0, 2), "中x");
    }
}
