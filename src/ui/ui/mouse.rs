use super::*;

impl MyApp {
    pub fn set_mouse_icon(&mut self, position: PhysicalPosition<f64>) -> bool {
        let previous_icon = self.mouse_icon;

        // Keep hit-testing aligned with hover logic to avoid edge-case mismatches.
        self.mouse_icon = if self.is_position_on_scroll_indicator(position)
            || self.tab_index_at_position(position).is_some()
        {
            CursorIcon::Default
        } else {
            CursorIcon::Text
        };

        if self.mouse_icon != previous_icon {
            self.window.set_cursor(self.mouse_icon);
            return true;
        }

        false
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> bool {
        let previous_hovered_tab = self.tab_index_at_position(self.mouse_position);
        let new_hovered_tab = self.tab_index_at_position(position);

        self.mouse_position = position;

        let mut needs_redraw = false;

        // 🔥 EDGE-BASED ACTIVATION (key fix)
        let near_edge = position.x >= (self.renderer.width as f64 - 12.0);

        if near_edge {
            self.mark_scroll_indicator_interaction_throttled();
            needs_redraw = true;
        }

        // Optional: precise hover detection (for cursor, etc.)
        if let Some(info) = self.visible_scroll_indicator_info() {
            if self.is_position_on_scroll_indicator_with_info(position, &info) {
                needs_redraw = true;
            }
        }

        let cursor_changed = self.set_mouse_icon(position);

        if self.dragging_scroll_indicator {
            self.handle_scroll_indicator_drag(position);
            return true;
        }

        if self.selecting {
            let scrolled = self.auto_scroll_during_selection(position);
            let selection_changed = self.handle_text_selection(position);
            if scrolled || selection_changed {
                self.sync_renderer_from_terminal(true);
                return true;
            }
        }

        // mouse reporting (unchanged)
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let mode = tab.terminal.performer.mouse_mode;
            let should_report = match mode {
                MouseMode::ButtonEvent => self.mouse_button_held.is_some(),
                MouseMode::AnyEvent => true,
                _ => false,
            };

            if should_report {
                let cell_x = ((position.x - TERMINAL_PADDING_X as f64)
                    / self.renderer.cell_width as f64) as usize;
                let cell_y = ((position.y - TAB_HEIGHT as f64 - TERMINAL_PADDING_Y as f64)
                    / self.renderer.line_height as f64) as usize;

                let btn_code = match self.mouse_button_held {
                    Some(MouseButton::Left) => 32,
                    Some(MouseButton::Middle) => 33,
                    Some(MouseButton::Right) => 34,
                    _ => 35,
                };

                tab.terminal
                    .performer
                    .report_mouse(cell_x, cell_y, btn_code, true);

                if let Some(tx) = &tab.tx {
                    for reply in tab.terminal.performer.drain_pty_replies() {
                        let _ = tx.send(PtyInput::Data(reply));
                    }
                }

                return true;
            }
        }

        if previous_hovered_tab == new_hovered_tab && !cursor_changed && !needs_redraw {
            return false;
        }

        self.sync_renderer_from_terminal(true);

        true
    }

    pub fn is_position_on_scroll_indicator_with_info(
        &self,
        position: PhysicalPosition<f64>,
        info: &ScrollIndicatorRenderInfo,
    ) -> bool {
        let indicator_x =
            self.renderer.width as f64 - INDICATOR_WIDTH as f64 - TERMINAL_PADDING_X as f64;

        position.x >= indicator_x
            && position.x <= indicator_x + INDICATOR_WIDTH as f64
            && position.y >= info.position_y as f64
            && position.y <= (info.position_y + info.height) as f64
    }

    pub fn is_position_on_scroll_indicator(&self, position: PhysicalPosition<f64>) -> bool {
        self.visible_scroll_indicator_info()
            .map(|info| self.is_position_on_scroll_indicator_with_info(position, &info))
            .unwrap_or(false)
    }

    pub fn handle_mouse_click(&mut self, state: ElementState, button: MouseButton) {
        match state {
            ElementState::Pressed => {
                self.mouse_button_held = Some(button);
                self.mouse_hold_start = Some(Instant::now());
                if button == MouseButton::Left {
                    if self.is_position_on_scroll_indicator(self.mouse_position) {
                        self.dragging_scroll_indicator = true;
                        self.drag_start_y = self.mouse_position.y;
                        self.drag_start_scroll_offset = self.scroll_offset;
                        self.mark_scroll_indicator_interaction();
                    } else if self.tab_index_at_position(self.mouse_position).is_some() {
                        self.handle_tab_click(self.mouse_position);
                    } else {
                        let cell = self.cell_position_from_mouse(self.mouse_position);
                        let now = Instant::now();
                        let within_multi_click_window =
                            match (self.last_left_click_at, self.last_left_click_cell, cell) {
                                (Some(last_at), Some((_, last_row)), Some((_, row))) => {
                                    row == last_row
                                        && now.duration_since(last_at) <= Duration::from_millis(350)
                                }
                                _ => false,
                            };

                        self.left_click_streak = if within_multi_click_window {
                            self.left_click_streak.saturating_add(1).min(3)
                        } else {
                            1
                        };

                        if self.left_click_streak == 2 {
                            if let Some((col, row)) = cell {
                                self.select_word(row, col);
                            }
                            self.selecting = false;
                        } else if self.left_click_streak >= 3 {
                            if let Some((_, row)) = cell {
                                self.select_row(row);
                            }
                            self.selecting = false;
                        } else {
                            self.selection_start = cell;
                            self.selection_end = None;
                            self.selecting = cell.is_some();
                        }

                        self.last_left_click_at = Some(now);
                        self.last_left_click_cell = cell;
                    }
                }
            }
            ElementState::Released => {
                self.mouse_button_held = None;
                self.mouse_hold_start = None;
                self.dragging_scroll_indicator = false;

                if button == MouseButton::Left
                    && self.selecting
                    && self.selection_start.is_some()
                    && self.selection_end.is_none()
                {
                    // Treat plain click as caret action only; no persistent text selection.
                    self.clear_selection();
                }

                self.selecting = false;
            }
        }

        // Mouse reporting
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.terminal.performer.mouse_mode != MouseMode::None {
                let btn_code = match button {
                    MouseButton::Left => 0u8,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => 3,
                };
                let pressed = state == ElementState::Pressed;

                // Pixel pozisyonunu hücre koordinatına çevir
                let cell_x = ((self.mouse_position.x - TERMINAL_PADDING_X as f64)
                    / self.renderer.cell_width as f64) as usize;
                let cell_y =
                    ((self.mouse_position.y - TAB_HEIGHT as f64 - TERMINAL_PADDING_Y as f64)
                        / self.renderer.line_height as f64) as usize;

                tab.terminal
                    .performer
                    .report_mouse(cell_x, cell_y, btn_code, pressed);

                if let Some(tx) = &tab.tx {
                    for reply in tab.terminal.performer.drain_pty_replies() {
                        let _ = tx.send(PtyInput::Data(reply));
                    }
                }
            }
        }

        self.sync_renderer_from_terminal(true);
    }

    fn handle_text_selection(&mut self, position: PhysicalPosition<f64>) -> bool {
        let Some(cell) = self.selection_cell_position_from_mouse(position) else {
            return false;
        };

        if self.selection_end.is_none() && self.selection_start == Some(cell) {
            return false;
        }

        if self.selection_end == Some(cell) {
            return false;
        }

        self.selection_end = Some(cell);
        true
    }

    fn auto_scroll_during_selection(&mut self, position: PhysicalPosition<f64>) -> bool {
        let Some(active_tab) = self.normalize_active_tab() else {
            return false;
        };

        let max_offset = self.tabs[active_tab].terminal.performer.scrollback.len() as i32;
        if max_offset == 0 {
            return false;
        }

        let content_top = TAB_HEIGHT as f64 + TERMINAL_PADDING_Y as f64;
        let content_bottom = self.renderer.height as f64 - TERMINAL_PADDING_Y as f64;
        if content_bottom <= content_top {
            return false;
        }

        let edge_threshold = self.renderer.line_height.max(8.0) as f64;
        let mut next_offset = self.scroll_offset;

        if position.y < content_top + edge_threshold {
            next_offset = (self.scroll_offset + 1).min(max_offset);
        } else if position.y > content_bottom - edge_threshold {
            next_offset = (self.scroll_offset - 1).max(0);
        }

        if next_offset == self.scroll_offset {
            return false;
        }

        self.scroll_offset = next_offset;
        self.mark_scroll_indicator_interaction_throttled();
        true
    }

    fn cell_position_from_mouse(&self, position: PhysicalPosition<f64>) -> Option<(usize, usize)> {
        if self.tabs.is_empty()
            || position.y < TAB_HEIGHT as f64 + TERMINAL_PADDING_Y as f64
            || position.x < TERMINAL_PADDING_X as f64
            || self.renderer.cell_width <= 0.0
            || self.renderer.line_height <= 0.0
        {
            return None;
        }

        let visible_rows = self.renderer.visible_row_capacity();
        if visible_rows == 0 {
            return None;
        }

        let rows = self.tabs[self.active_tab]
            .terminal
            .visible_rows(self.scroll_offset, visible_rows);
        if rows.is_empty() {
            return None;
        }

        let performer = &self.tabs[self.active_tab].terminal.performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return None;
        }

        let offset = self.scroll_offset.max(0) as usize;
        let end = total_rows.saturating_sub(offset);
        let start = end.saturating_sub(rows.len());

        let col = ((position.x - TERMINAL_PADDING_X as f64) / self.renderer.cell_width as f64)
            .floor()
            .max(0.0) as usize;
        let row = ((position.y - TAB_HEIGHT as f64 - TERMINAL_PADDING_Y as f64)
            / self.renderer.line_height as f64)
            .floor()
            .max(0.0) as usize;

        let row = row.min(rows.len() - 1);
        let max_cols = rows[row].len().max(1);
        let col = col.min(max_cols - 1);

        Some((col, (start + row).min(total_rows - 1)))
    }

    fn selection_cell_position_from_mouse(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(usize, usize)> {
        if self.tabs.is_empty()
            || self.renderer.cell_width <= 0.0
            || self.renderer.line_height <= 0.0
        {
            return None;
        }

        let visible_rows = self.renderer.visible_row_capacity();
        if visible_rows == 0 {
            return None;
        }

        let rows = self.tabs[self.active_tab]
            .terminal
            .visible_rows(self.scroll_offset, visible_rows);
        if rows.is_empty() {
            return None;
        }

        let performer = &self.tabs[self.active_tab].terminal.performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return None;
        }

        let offset = self.scroll_offset.max(0) as usize;
        let end = total_rows.saturating_sub(offset);
        let start = end.saturating_sub(rows.len());

        let content_left = TERMINAL_PADDING_X as f64;
        let content_top = TAB_HEIGHT as f64 + TERMINAL_PADDING_Y as f64;
        let content_bottom = content_top + rows.len() as f64 * self.renderer.line_height as f64;

        let clamped_x = position.x.max(content_left);
        let clamped_y = position
            .y
            .clamp(content_top, (content_bottom - 1.0).max(content_top));

        let col = ((clamped_x - TERMINAL_PADDING_X as f64) / self.renderer.cell_width as f64)
            .floor()
            .max(0.0) as usize;
        let row = ((clamped_y - TAB_HEIGHT as f64 - TERMINAL_PADDING_Y as f64)
            / self.renderer.line_height as f64)
            .floor()
            .max(0.0) as usize;

        let row = row.min(rows.len() - 1);
        let max_cols = rows[row].len().max(1);
        let col = col.min(max_cols - 1);

        Some((col, (start + row).min(total_rows - 1)))
    }

    fn select_row(&mut self, abs_row: usize) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            self.selection_start = None;
            self.selection_end = None;
            return;
        };

        let performer = &tab.terminal.performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 || abs_row >= total_rows {
            self.selection_start = None;
            self.selection_end = None;
            return;
        }

        let scrollback_len = performer.scrollback.len();
        let row_len = if abs_row < scrollback_len {
            performer.scrollback[abs_row].len()
        } else {
            performer.grid[abs_row - scrollback_len].len()
        };

        if row_len == 0 {
            self.selection_start = None;
            self.selection_end = None;
            return;
        }

        self.selection_start = Some((0, abs_row));
        self.selection_end = Some((row_len - 1, abs_row));
    }

    fn select_word(&mut self, abs_row: usize, abs_col: usize) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            self.clear_selection();
            return;
        };

        let performer = &tab.terminal.performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 || abs_row >= total_rows {
            self.clear_selection();
            return;
        }

        let scrollback_len = performer.scrollback.len();
        let row = if abs_row < scrollback_len {
            &performer.scrollback[abs_row]
        } else {
            &performer.grid[abs_row - scrollback_len]
        };

        if row.is_empty() {
            self.clear_selection();
            return;
        }

        let col = abs_col.min(row.len() - 1);
        let clicked = row[col].c;
        let class = char_class(clicked);

        let mut start = col;
        while start > 0 && char_class(row[start - 1].c) == class {
            start -= 1;
        }

        let mut end = col;
        while end + 1 < row.len() && char_class(row[end + 1].c) == class {
            end += 1;
        }

        self.selection_start = Some((start, abs_row));
        self.selection_end = Some((end, abs_row));
    }

    fn handle_scroll_indicator_drag(&mut self, position: PhysicalPosition<f64>) {
        let Some(active_tab) = self.normalize_active_tab() else {
            return;
        };

        let tab_bar_height = TAB_HEIGHT as f32;
        let viewport_height = self.renderer.height as f32;
        let usable_height = viewport_height - tab_bar_height;

        let scrollback_len = self.tabs[active_tab].terminal.performer.scrollback.len() as f32;
        let visible_lines = self.tabs[active_tab].terminal.performer.rows as f32;
        let total_lines = scrollback_len + visible_lines;

        let raw_height = (visible_lines / total_lines) * usable_height;
        let indicator_height = raw_height.max(16.0).min(usable_height);
        let scrollable_track = (usable_height - indicator_height).max(0.0);

        if scrollable_track <= 0.0 {
            return;
        }

        let delta_y = position.y - self.drag_start_y;
        let ratio_delta = delta_y as f32 / scrollable_track;
        let offset_delta = -(ratio_delta * scrollback_len) as i32;

        let max_offset = scrollback_len as i32;
        let new_offset = (self.drag_start_scroll_offset + offset_delta)
            .max(0)
            .min(max_offset);
        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            self.mark_scroll_indicator_interaction_throttled();
        }

        self.sync_renderer_from_terminal(false);
    }

    fn handle_tab_click(&mut self, position: PhysicalPosition<f64>) {
        let Some(mut tabs) = self.visible_tab_info(self.tabs.len()) else {
            return;
        };

        for (index, tab) in tabs.iter_mut().enumerate() {
            if self.is_mouse_on_tab(position, tab) {
                if self.active_tab != index {
                    self.active_tab = index;
                    self.clear_selection();
                    self.reset_scrollback_view();
                    self.sync_renderer_from_terminal(true);
                }
                return;
            }
        }
    }

    pub(super) fn is_mouse_on_tab(
        &self,
        position: PhysicalPosition<f64>,
        tab: &mut TabRenderInfo,
    ) -> bool {
        position.x >= tab.x as f64
            && position.x < (tab.x + tab.width) as f64
            && position.y >= tab.y as f64
            && position.y < (tab.y + tab.height) as f64
    }

    pub(super) fn tab_index_at_position(&self, position: PhysicalPosition<f64>) -> Option<usize> {
        if self.tabs.is_empty() || position.y < 0.0 || position.y >= TAB_HEIGHT as f64 {
            return None;
        }

        let tab_width = self.renderer.width as f64 / self.tabs.len() as f64;
        if tab_width <= 0.0 {
            return None;
        }

        let index = (position.x / tab_width).floor() as isize;
        if index < 0 || index as usize >= self.tabs.len() {
            return None;
        }

        Some(index as usize)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Whitespace,
    Symbol,
}

fn char_class(c: char) -> CharClass {
    if c.is_ascii_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Symbol
    }
}
