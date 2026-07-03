use super::*;
use crate::terminal::Cell;

impl MyApp {
    pub(super) fn build_decorated_visible_rows(&self, active_tab: usize) -> Vec<Vec<Cell>> {
        let visible_rows = self.renderer.visible_row_capacity();
        let base_rows = self.session.tabs[active_tab]
            .terminal
            .visible_rows(self.session.scroll_offset, visible_rows);

        let mut rows: Vec<Vec<Cell>> = base_rows.iter().map(|row| (*row).clone()).collect();
        if rows.is_empty() {
            return rows;
        }

        let terminal = &self.session.tabs[active_tab].terminal;
        let Some(window) = terminal.visible_row_window(self.session.scroll_offset, rows.len())
        else {
            return rows;
        };

        if self.interaction.link_settings.enable_hover_underline
            && let Some(link) = self.hovered_link_span_at_mouse()
                && link.abs_row >= window.start && link.abs_row < window.end {
                    let rel_row = link.abs_row - window.start;
                    if let Some(row) = rows.get_mut(rel_row)
                        && !row.is_empty() {
                            let c0 = link.start_col.min(row.len() - 1);
                            let c1 = link.end_col.min(row.len() - 1);
                            for cell in row.iter_mut().take(c1 + 1).skip(c0) {
                                cell.is_link_hovered = true;
                            }
                        }
                }

        if let Some(range) = self.current_selection_range(window.total_rows) {
            for abs_row in range.start.1..=range.end.1 {
                if abs_row < window.start || abs_row >= window.end {
                    continue;
                }
                let rel_row = abs_row - window.start;
                let Some(row) = rows.get_mut(rel_row) else {
                    continue;
                };
                if row.is_empty() {
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
                    continue;
                }
                for cell in row.iter_mut().take(row_end + 1).skip(row_start) {
                    cell.is_selected = true;
                }
            }
        }

        if self.session.scroll_offset == 0
            && !terminal.performer.in_alt_screen
            && let Some(preview) = self.current_history_preview()
        {
                let cursor_row = terminal.performer.cursor_y;
                let cursor_x = terminal.performer.cursor_x;
                if let Some(row) = rows.get_mut(cursor_row) {
                    super::history_completion::draw_history_preview(row, cursor_x, preview);
                }
            }

        rows
    }
}
