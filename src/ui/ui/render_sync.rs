use super::*;

impl MyApp {
    pub fn sync_renderer_from_terminal(&mut self, content_changed: bool) {
        self.apply_selection_to_cells();
        let tabs = self.visible_tab_info(self.tabs.len());
        let Some(active_tab) = self.normalize_active_tab() else {
            self.renderer
                .set_cells(&[], None, tabs, None, content_changed);
            return;
        };

        let visible_rows = self.renderer.visible_row_capacity();
        let rows = self.tabs[active_tab]
            .terminal
            .visible_rows(self.scroll_offset, visible_rows);

        let cursor = self.visible_cursor_info(visible_rows, rows.len());
        let scroll_indicator = self.visible_scroll_indicator_info();
        self.window.set_cursor(self.mouse_icon);

        self.renderer
            .set_cells(&rows, cursor, tabs, scroll_indicator, content_changed);
    }

    fn apply_selection_to_cells(&mut self) {
        let Some(active_tab) = self.normalize_active_tab() else {
            return;
        };

        let performer = &mut self.tabs[active_tab].terminal.performer;
        for row in performer.scrollback.iter_mut() {
            for cell in row.iter_mut() {
                cell.is_selected = false;
            }
        }
        for row in performer.grid.iter_mut() {
            for cell in row.iter_mut() {
                cell.is_selected = false;
            }
        }

        let (Some((sx, sy)), Some((ex, ey))) = (self.selection_start, self.selection_end) else {
            return;
        };

        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return;
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

        for abs_row in a_row..=b_row {
            let row = if abs_row < scrollback_len {
                &mut performer.scrollback[abs_row]
            } else {
                &mut performer.grid[abs_row - scrollback_len]
            };

            if row.is_empty() {
                continue;
            }

            let row_start = if abs_row == a_row { a_col } else { 0 };
            let row_end = if abs_row == b_row {
                b_col.min(row.len() - 1)
            } else {
                row.len() - 1
            };

            if row_start > row_end {
                continue;
            }

            for col in row_start..=row_end {
                row[col].is_selected = true;
            }
        }
    }

    pub(super) fn visible_tab_info(&self, tab_count: usize) -> Option<Vec<TabRenderInfo>> {
        if tab_count == 0 || self.active_tab >= tab_count {
            return None;
        }

        let tab_index = self.active_tab;
        let tab_id = self.tabs.get(tab_index)?.id;

        let tab_width = self.renderer.width as f32 / tab_count as f32;

        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let title = if tab.terminal.performer.title.is_empty() {
                    "~".to_string()
                } else {
                    tab.terminal.performer.title.clone()
                };

                TabRenderInfo {
                    title,
                    is_hovered: (self.mouse_position.x >= i as f64 * tab_width as f64)
                        && (self.mouse_position.x < (i as f64 + 1.0) * tab_width as f64)
                        && (self.mouse_position.y >= 0.0)
                        && (self.mouse_position.y < TAB_HEIGHT as f64),
                    x: (i as f32 * tab_width).round() as usize,
                    y: 0,
                    width: tab_width.round() as usize,
                    height: TAB_HEIGHT,
                    active: tab.id == tab_id,
                }
            })
            .collect::<Vec<_>>()
            .into()
    }

    pub(super) fn visible_cursor_info(
        &self,
        requested_visible_rows: usize,
        actual_visible_rows: usize,
    ) -> Option<CursorRenderInfo> {
        let tab = self.tabs.get(self.active_tab)?;

        if !self.cursor_render_visible() {
            return None;
        }

        let scrollback_len = tab.terminal.performer.scrollback.len();
        let grid_len = tab.terminal.performer.grid.len();
        let total_rows = scrollback_len + grid_len;
        if total_rows == 0 {
            return None;
        }

        let offset = self.scroll_offset.max(0) as usize;
        let end = total_rows.saturating_sub(offset);
        let start = end.saturating_sub(requested_visible_rows);

        let cursor_abs_row = scrollback_len + tab.terminal.performer.cursor_y;
        if cursor_abs_row < start || cursor_abs_row >= end {
            return None;
        }

        let cursor_row = cursor_abs_row - start;
        if cursor_row >= actual_visible_rows {
            return None;
        }

        let cursor_style = if self.has_focus {
            match tab.terminal.performer.cursor_style {
                CursorStyle::Block => CursorRenderStyle::Block,
                CursorStyle::Underline => CursorRenderStyle::Underline,
                CursorStyle::Bar => CursorRenderStyle::Bar,
            }
        } else {
            CursorRenderStyle::Unfocused
        };

        let blink_on = if self.has_focus {
            !self.cursor_blink_active() || self.cursor_blink_on
        } else {
            true
        };

        Some(CursorRenderInfo {
            col: tab.terminal.performer.cursor_x,
            row: cursor_row,
            style: cursor_style,
            color: CURSOR_COLOR,
            blink_on,
        })
    }
}
