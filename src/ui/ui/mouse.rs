use super::*;
use std::process::Command;

const CHAR_SELECTION_DRAG_THRESHOLD_PX: f64 = 4.0;

#[derive(PartialEq)]
pub(crate) enum SelectionMode {
    Char,
    Word,
    Line,
}

impl MyApp {
    fn hyperlink_features_enabled(&self) -> bool {
        if !self.interaction.link_settings.enable_hyperlinks {
            return false;
        }
        let in_alt = self
            .active_tab()
            .map(|tab| tab.terminal.performer.in_alt_screen)
            .unwrap_or(false);
        !(self.interaction.link_settings.disable_in_alt_screen && in_alt)
    }

    fn plaintext_links_enabled(&self) -> bool {
        self.hyperlink_features_enabled() && self.interaction.link_settings.enable_plaintext_links
    }

    pub fn set_mouse_icon(&mut self, position: PhysicalPosition<f64>) -> bool {
        let previous_icon = self.interaction.mouse_icon;

        // Keep hit-testing aligned with hover logic to avoid edge-case mismatches.
        self.interaction.mouse_icon = if self.is_position_on_scroll_indicator(position)
            || self.tab_index_at_position(position).is_some()
            || self.is_position_in_settings_ui(position)
        {
            CursorIcon::Default
        } else if self.link_span_at_mouse_position(position).is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Text
        };

        if self.interaction.mouse_icon != previous_icon {
            self.window.set_cursor(self.interaction.mouse_icon);
            return true;
        }

        false
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> bool {
        let previous_hovered_tab = self.tab_index_at_position(self.interaction.mouse_position);
        let new_hovered_tab = self.tab_index_at_position(position);

        self.interaction.mouse_position = position;

        let mut needs_redraw = false;

        let near_edge = position.x >= (self.renderer.width as f64 - 12.0);

        if near_edge {
            self.mark_scroll_indicator_interaction_throttled();
            needs_redraw = true;
        }

        if let Some(info) = self.visible_scroll_indicator_info()
            && self.is_position_on_scroll_indicator_with_info(position, &info)
        {
            needs_redraw = true;
        }

        let cursor_changed = self.set_mouse_icon(position);

        if self.interaction.dragging_scroll_indicator {
            self.handle_scroll_indicator_drag(position);
            return true;
        }

        if self.interaction.mouse_button_held == Some(MouseButton::Left)
            && !self.interaction.selecting
            && self.interaction.selection_mode == SelectionMode::Char
            && self.interaction.selection_anchor.is_some()
            && let Some(press_position) = self.interaction.left_press_position
        {
            let dx = position.x - press_position.x;
            let dy = position.y - press_position.y;
            if dx * dx + dy * dy
                >= CHAR_SELECTION_DRAG_THRESHOLD_PX * CHAR_SELECTION_DRAG_THRESHOLD_PX
            {
                self.interaction.selecting = true;
                self.interaction.selection_start = self.interaction.selection_anchor;
            }
        }

        if self.interaction.selecting {
            let scrolled = self.auto_scroll_during_selection(position);
            let selection_changed = self.handle_text_selection(position);
            if scrolled || selection_changed {
                self.sync_renderer_from_terminal(true);
                return true;
            }
        }

        let (mouse_cell_x, mouse_cell_y) = renderer_cell_from_position(
            position,
            self.renderer.cell_width,
            self.renderer.line_height,
        );
        let held_button = self.interaction.mouse_button_held;

        if let Some(tab) = self.active_tab_mut() {
            let mode = tab.terminal.performer.mouse_mode;
            let should_report = match mode {
                MouseMode::ButtonEvent => held_button.is_some(),
                MouseMode::AnyEvent => true,
                _ => false,
            };

            if should_report {
                let btn_code = match held_button {
                    Some(MouseButton::Left) => 32,
                    Some(MouseButton::Middle) => 33,
                    Some(MouseButton::Right) => 34,
                    _ => 35,
                };

                tab.terminal
                    .performer
                    .report_mouse(mouse_cell_x, mouse_cell_y, btn_code, true);

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
                self.interaction.mouse_button_held = Some(button);
                self.interaction.mouse_hold_start = Some(Instant::now());
                if button == MouseButton::Left {
                    if self.handle_settings_click(self.interaction.mouse_position) {
                        self.sync_renderer_from_terminal(true);
                        return;
                    }

                    if self.modifiers.super_key()
                        && self.interaction.link_settings.enable_cmd_click_open
                        && let Some(link) = self.hovered_link_span_at_mouse()
                    {
                        let _ = open_url_in_system_browser(&normalize_url_for_open(&link.url));
                        return;
                    }

                    self.interaction.left_press_position = Some(self.interaction.mouse_position);
                    if self.is_position_on_scroll_indicator(self.interaction.mouse_position) {
                        self.interaction.dragging_scroll_indicator = true;
                        self.interaction.drag_start_y = self.interaction.mouse_position.y;
                        self.interaction.drag_start_scroll_offset = self.session.scroll_offset;
                        self.mark_scroll_indicator_interaction();
                    } else if self
                        .tab_index_at_position(self.interaction.mouse_position)
                        .is_some()
                    {
                        self.handle_tab_click(self.interaction.mouse_position);
                    } else {
                        let cell = self.cell_position_from_mouse(self.interaction.mouse_position);
                        let now = Instant::now();
                        let within_multi_click_window = match (
                            self.interaction.last_left_click_at,
                            self.interaction.last_left_click_cell,
                            cell,
                        ) {
                            (Some(last_at), Some((_, last_row)), Some((_, row))) => {
                                row == last_row
                                    && now.duration_since(last_at) <= Duration::from_millis(350)
                            }
                            _ => false,
                        };

                        self.interaction.left_click_streak = if within_multi_click_window {
                            self.interaction.left_click_streak.saturating_add(1).min(3)
                        } else {
                            1
                        };

                        if self.interaction.left_click_streak == 2 {
                            if let Some((col, row)) = cell {
                                self.select_word(row, col);
                                self.interaction.selection_anchor =
                                    self.interaction.selection_start;
                            }
                            self.interaction.selection_mode = SelectionMode::Word;
                            self.interaction.selecting = true;
                        } else if self.interaction.left_click_streak >= 3 {
                            if let Some((_, row)) = cell {
                                self.select_row(row);
                                self.interaction.selection_anchor =
                                    self.interaction.selection_start;
                            }
                            self.interaction.selection_mode = SelectionMode::Line;
                            self.interaction.selecting = true;
                        } else {
                            self.interaction.selection_anchor = cell;
                            self.interaction.selection_start = None;
                            self.interaction.selection_end = None;
                            self.interaction.selection_mode = SelectionMode::Char;
                        }

                        let in_alt = self
                            .active_tab()
                            .map(|tab| tab.terminal.performer.in_alt_screen)
                            .unwrap_or(false);

                        if in_alt {
                            self.interaction.selection_start = None;
                            self.interaction.selection_end = None;
                            self.interaction.selecting = false;
                            self.interaction.selection_anchor = None;
                        }
                        self.interaction.last_left_click_at = Some(now);
                        self.interaction.last_left_click_cell = cell;
                    }
                }
            }
            ElementState::Released => {
                self.interaction.mouse_button_held = None;
                self.interaction.mouse_hold_start = None;
                self.interaction.left_press_position = None;
                self.interaction.dragging_scroll_indicator = false;

                if self.interaction.selection_mode == SelectionMode::Char
                    && !self.interaction.selecting
                {
                    self.interaction.selection_start = None;
                    self.interaction.selection_end = None;
                    self.interaction.selection_anchor = None;
                }

                self.interaction.selecting = false;
            }
        }

        let (mouse_cell_x, mouse_cell_y) = renderer_cell_from_position(
            self.interaction.mouse_position,
            self.renderer.cell_width,
            self.renderer.line_height,
        );

        if let Some(tab) = self.active_tab_mut()
            && tab.terminal.performer.mouse_mode != MouseMode::None
        {
            let btn_code = match button {
                MouseButton::Left => 0u8,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => 3,
            };
            let pressed = state == ElementState::Pressed;

            tab.terminal
                .performer
                .report_mouse(mouse_cell_x, mouse_cell_y, btn_code, pressed);

            if let Some(tx) = &tab.tx {
                for reply in tab.terminal.performer.drain_pty_replies() {
                    let _ = tx.send(PtyInput::Data(reply));
                }
            }
        }

        self.sync_renderer_from_terminal(true);
    }

    fn handle_text_selection(&mut self, position: PhysicalPosition<f64>) -> bool {
        let Some(cell) = self.selection_cell_position_from_mouse(position) else {
            return false;
        };

        match self.interaction.selection_mode {
            SelectionMode::Char => {
                if self.interaction.selection_end == Some(cell) {
                    return false;
                }
                self.interaction.selection_end = Some(cell);
            }

            SelectionMode::Word => {
                let (col, row) = cell;
                let anchor = self.interaction.selection_anchor;

                self.select_word(row, col);

                if let Some(anchor_pos) = anchor {
                    let anchor_row = anchor_pos.1;
                    let anchor_col = anchor_pos.0;

                    let dragging_forward = if row != anchor_row {
                        row > anchor_row
                    } else {
                        col >= anchor_col
                    };

                    if dragging_forward {
                        self.interaction.selection_start = Some(anchor_pos);
                    } else {
                        let word_start = self.interaction.selection_start;

                        self.select_word(anchor_row, anchor_col);
                        let anchor_end = self.interaction.selection_end;

                        self.interaction.selection_start = word_start;
                        self.interaction.selection_end = anchor_end;
                    }
                }
            }

            SelectionMode::Line => {
                let (_, row) = cell;
                let anchor = self.interaction.selection_anchor;

                if let Some(anchor_pos) = anchor {
                    let anchor_row = anchor_pos.1;

                    if row >= anchor_row {
                        self.select_row(row);
                        let new_end = self.interaction.selection_end;
                        self.interaction.selection_start = Some((0, anchor_row));
                        self.interaction.selection_end = new_end;
                    } else {
                        self.select_row(anchor_row);
                        let anchor_end = self.interaction.selection_end;

                        self.select_row(row);

                        self.interaction.selection_end = anchor_end;
                    }
                }
            }
        }

        true
    }

    fn auto_scroll_during_selection(&mut self, position: PhysicalPosition<f64>) -> bool {
        let Some(active_tab) = self.normalize_active_tab() else {
            return false;
        };

        let max_offset = self.session.tabs[active_tab]
            .terminal
            .performer
            .scrollback
            .len() as i32;
        if max_offset == 0 {
            return false;
        }

        let content_top = TAB_HEIGHT as f64 + TERMINAL_PADDING_Y as f64;
        let content_bottom = self.renderer.height as f64 - TERMINAL_PADDING_Y as f64;
        if content_bottom <= content_top {
            return false;
        }

        let edge_threshold = self.renderer.line_height.max(8.0) as f64;
        let mut next_offset = self.session.scroll_offset;

        if position.y < content_top + edge_threshold {
            next_offset = (self.session.scroll_offset + 1).min(max_offset);
        } else if position.y > content_bottom - edge_threshold {
            next_offset = (self.session.scroll_offset - 1).max(0);
        }

        if next_offset == self.session.scroll_offset {
            return false;
        }

        self.session.scroll_offset = next_offset;
        self.mark_scroll_indicator_interaction_throttled();
        true
    }

    fn cell_position_from_mouse(&self, position: PhysicalPosition<f64>) -> Option<(usize, usize)> {
        if self.session.tabs.is_empty()
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

        let rows = self.session.tabs[self.session.active_tab]
            .terminal
            .visible_rows(self.session.scroll_offset, visible_rows);
        if rows.is_empty() {
            return None;
        }

        let performer = &self.session.tabs[self.session.active_tab]
            .terminal
            .performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return None;
        }

        let offset = self.session.scroll_offset.max(0) as usize;
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

    pub(super) fn hovered_link_span_at_mouse(&self) -> Option<LinkSpan> {
        if !self.hyperlink_features_enabled() {
            return None;
        }
        self.link_span_at_mouse_position(self.interaction.mouse_position)
    }

    fn link_span_at_mouse_position(&self, position: PhysicalPosition<f64>) -> Option<LinkSpan> {
        if !self.hyperlink_features_enabled() {
            return None;
        }
        let (col, abs_row) = self.cell_position_from_mouse(position)?;
        self.link_span_at_cell(abs_row, col)
    }

    fn link_span_at_cell(&self, abs_row: usize, col: usize) -> Option<LinkSpan> {
        let tab = self.session.tabs.get(self.session.active_tab)?;
        let performer = &tab.terminal.performer;
        let scrollback_len = performer.scrollback.len();

        let row = if abs_row < scrollback_len {
            performer.scrollback.get(abs_row)?
        } else {
            performer.grid.get(abs_row.saturating_sub(scrollback_len))?
        };

        if row.is_empty() || col >= row.len() {
            return None;
        }

        link_span_in_row_at_col(row, col, self.plaintext_links_enabled()).map(
            |(start_col, end_col, url)| LinkSpan {
                abs_row,
                start_col,
                end_col,
                url,
            },
        )
    }

    fn selection_cell_position_from_mouse(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(usize, usize)> {
        if self.session.tabs.is_empty()
            || self.renderer.cell_width <= 0.0
            || self.renderer.line_height <= 0.0
        {
            return None;
        }

        let visible_rows = self.renderer.visible_row_capacity();
        if visible_rows == 0 {
            return None;
        }

        let rows = self.session.tabs[self.session.active_tab]
            .terminal
            .visible_rows(self.session.scroll_offset, visible_rows);
        if rows.is_empty() {
            return None;
        }

        let performer = &self.session.tabs[self.session.active_tab]
            .terminal
            .performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 {
            return None;
        }

        let offset = self.session.scroll_offset.max(0) as usize;
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
        let Some(tab) = self.session.tabs.get(self.session.active_tab) else {
            self.interaction.selection_start = None;
            self.interaction.selection_end = None;
            return;
        };

        let performer = &tab.terminal.performer;
        let total_rows = performer.scrollback.len() + performer.grid.len();
        if total_rows == 0 || abs_row >= total_rows {
            self.interaction.selection_start = None;
            self.interaction.selection_end = None;
            return;
        }

        let scrollback_len = performer.scrollback.len();
        let row_len = if abs_row < scrollback_len {
            performer.scrollback[abs_row].len()
        } else {
            performer.grid[abs_row - scrollback_len].len()
        };

        if row_len == 0 {
            self.interaction.selection_start = None;
            self.interaction.selection_end = None;
            return;
        }

        self.interaction.selection_start = Some((0, abs_row));
        self.interaction.selection_end = Some((row_len - 1, abs_row));
    }

    fn select_word(&mut self, abs_row: usize, abs_col: usize) {
        let Some(tab) = self.session.tabs.get(self.session.active_tab) else {
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
        let clicked = cell_head_char(&row[col]);
        let class = char_class(clicked);

        let mut start = col;
        while start > 0 && char_class(cell_head_char(&row[start - 1])) == class {
            start -= 1;
        }

        let mut end = col;
        while end + 1 < row.len() && char_class(cell_head_char(&row[end + 1])) == class {
            end += 1;
        }

        self.interaction.selection_start = Some((start, abs_row));
        self.interaction.selection_end = Some((end, abs_row));
    }

    fn handle_scroll_indicator_drag(&mut self, position: PhysicalPosition<f64>) {
        let Some(active_tab) = self.normalize_active_tab() else {
            return;
        };

        let tab_bar_height = TAB_HEIGHT as f32;
        let viewport_height = self.renderer.height as f32;
        let usable_height = viewport_height - tab_bar_height;

        let scrollback_len = self.session.tabs[active_tab]
            .terminal
            .performer
            .scrollback
            .len() as f32;
        let visible_lines = self.session.tabs[active_tab].terminal.performer.rows as f32;
        let total_lines = scrollback_len + visible_lines;

        let raw_height = (visible_lines / total_lines) * usable_height;
        let indicator_height = raw_height.max(16.0).min(usable_height);
        let scrollable_track = (usable_height - indicator_height).max(0.0);

        if scrollable_track <= 0.0 {
            return;
        }

        let delta_y = position.y - self.interaction.drag_start_y;
        let ratio_delta = delta_y as f32 / scrollable_track;
        let offset_delta = -(ratio_delta * scrollback_len) as i32;

        let max_offset = scrollback_len as i32;
        let new_offset = (self.interaction.drag_start_scroll_offset + offset_delta)
            .max(0)
            .min(max_offset);
        if new_offset != self.session.scroll_offset {
            self.session.scroll_offset = new_offset;
            self.mark_scroll_indicator_interaction_throttled();
        }

        self.sync_renderer_from_terminal(false);
    }

    fn handle_tab_click(&mut self, position: PhysicalPosition<f64>) {
        let Some(mut tabs) = self.visible_tab_info(self.session.tabs.len()) else {
            return;
        };

        for (index, tab) in tabs.iter_mut().enumerate() {
            if self.is_mouse_on_tab(position, tab) {
                if self.session.active_tab != index {
                    self.session.active_tab = index;
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
        if self.session.tabs.is_empty() || position.y < 0.0 || position.y >= TAB_HEIGHT as f64 {
            return None;
        }

        let tab_width = self.renderer.width as f64 / self.session.tabs.len() as f64;
        if tab_width <= 0.0 {
            return None;
        }

        let index = (position.x / tab_width).floor() as isize;
        if index < 0 || index as usize >= self.session.tabs.len() {
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

fn cell_head_char(cell: &crate::terminal::Cell) -> char {
    cell.display_text().chars().next().unwrap_or(' ')
}

fn renderer_cell_from_position(
    position: PhysicalPosition<f64>,
    cell_width: f32,
    line_height: f32,
) -> (usize, usize) {
    let cell_x = ((position.x - TERMINAL_PADDING_X as f64) / cell_width as f64)
        .floor()
        .max(0.0) as usize;
    let cell_y = ((position.y - TAB_HEIGHT as f64 - TERMINAL_PADDING_Y as f64) / line_height as f64)
        .floor()
        .max(0.0) as usize;

    (cell_x, cell_y)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LinkSpan {
    pub abs_row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub url: String,
}

fn link_span_in_row_at_col(
    row: &[crate::terminal::Cell],
    mut col: usize,
    allow_plaintext_links: bool,
) -> Option<(usize, usize, String)> {
    if col >= row.len() || row.is_empty() {
        return None;
    }

    while col > 0 && row[col].wide_continuation {
        col -= 1;
    }

    if let Some(url) = row[col].hyperlink.as_ref() {
        let mut start = col;
        while start > 0 && row[start - 1].hyperlink.as_ref() == Some(url) {
            start -= 1;
        }

        let mut end = col;
        while end + 1 < row.len() && row[end + 1].hyperlink.as_ref() == Some(url) {
            end += 1;
        }

        while end + 1 < row.len() && row[end + 1].wide_continuation {
            end += 1;
        }

        return Some((start, end, url.to_string()));
    }

    if !allow_plaintext_links || is_link_break_cell(&row[col]) {
        return None;
    }

    let mut start = col;
    while start > 0 && !is_link_break_cell(&row[start - 1]) {
        start -= 1;
    }

    let mut end = col;
    while end + 1 < row.len() && !is_link_break_cell(&row[end + 1]) {
        end += 1;
    }

    while start <= end {
        let head = cell_head_char(&row[start]);
        if is_leading_trim_char(head) {
            start += 1;
        } else {
            break;
        }
    }

    while end >= start {
        let tail = cell_head_char(&row[end]);
        if is_trailing_trim_char(tail) {
            if end == 0 {
                break;
            }
            end -= 1;
        } else {
            break;
        }
    }

    if start > end {
        return None;
    }

    let token: String = row[start..=end].iter().map(|c| c.display_text()).collect();

    if !looks_like_url(&token) {
        return None;
    }

    Some((start, end, token))
}

fn is_link_break_cell(cell: &crate::terminal::Cell) -> bool {
    let t = cell.display_text();
    if t.is_empty() {
        return true;
    }
    t.chars().all(|c| c.is_whitespace())
}

fn is_leading_trim_char(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '<' | '"' | '\'')
}

fn is_trailing_trim_char(c: char) -> bool {
    matches!(
        c,
        ')' | ']' | '}' | '>' | ',' | '.' | ';' | ':' | '!' | '?' | '"' | '\''
    )
}

fn looks_like_url(token: &str) -> bool {
    let t = token.trim();
    if t.is_empty() {
        return false;
    }

    if t.contains("://") || t.starts_with("www.") {
        return true;
    }

    if t.starts_with("localhost") {
        return t == "localhost" || t.starts_with("localhost:") || t.starts_with("localhost/");
    }

    let host_port_prefix = t.split('/').next().unwrap_or(t);
    let ip_like_chars_only = host_port_prefix
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ':');
    let has_digit = host_port_prefix.chars().any(|c| c.is_ascii_digit());
    if ip_like_chars_only && has_digit && host_port_prefix.contains('.') {
        return true;
    }

    false
}

fn normalize_url_for_open(url: &str) -> String {
    if url.contains("://") {
        return url.to_string();
    }

    if url.starts_with("localhost") || url.starts_with("www.") {
        return format!("http://{url}");
    }

    let host_port_prefix = url.split('/').next().unwrap_or(url);
    if host_port_prefix.contains('.') {
        return format!("http://{url}");
    }

    url.to_string()
}

fn open_url_in_system_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open url: {e}"))
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open url: {e}"))
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open url: {e}"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        Err("opening urls is not supported on this platform".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{link_span_in_row_at_col, normalize_url_for_open};
    use crate::terminal::Cell;

    #[test]
    fn hyperlink_lookup_prefers_cell_and_falls_back_to_leading_wide_cell() {
        let row = vec![
            Cell {
                hyperlink: Some("https://example.com".to_string().into()),
                c: '中',
                text: "中".to_string().into(),
                wide_continuation: false,
                ..Default::default()
            },
            Cell {
                hyperlink: None,
                c: ' ',
                text: String::new().into(),
                wide_continuation: true,
                ..Default::default()
            },
        ];

        let s0 = link_span_in_row_at_col(&row, 0, true).expect("expected span");
        assert_eq!(s0.0, 0);
        assert_eq!(s0.1, 1);
        assert_eq!(s0.2, "https://example.com");

        let s1 = link_span_in_row_at_col(&row, 1, true).expect("expected span");
        assert_eq!(s1.0, 0);
        assert_eq!(s1.1, 1);
        assert_eq!(s1.2, "https://example.com");
    }

    #[test]
    fn plaintext_localhost_url_is_detected_and_normalized() {
        let row = vec![
            Cell {
                c: 'l',
                text: "localhost:3000/destek".to_string().into(),
                ..Default::default()
            },
            Cell::default(),
        ];

        let span = link_span_in_row_at_col(&row, 0, true).expect("expected localhost url span");
        assert_eq!(span.2, "localhost:3000/destek");
        assert_eq!(
            normalize_url_for_open(&span.2),
            "http://localhost:3000/destek"
        );
    }

    #[test]
    fn filenames_are_not_detected_as_links() {
        let row = vec![Cell {
            c: 'p',
            text: "package.json".to_string().into(),
            ..Default::default()
        }];
        assert!(link_span_in_row_at_col(&row, 0, true).is_none());

        let row2 = vec![Cell {
            c: 'R',
            text: "README.md".to_string().into(),
            ..Default::default()
        }];
        assert!(link_span_in_row_at_col(&row2, 0, true).is_none());
    }

    #[test]
    fn relative_paths_are_not_detected_as_links() {
        let row = vec![Cell {
            c: '.',
            text: "./crmFrontendNewModules/node_modules/@mui/material/ListItemSecondaryAction:"
                .to_string()
                .into(),
            ..Default::default()
        }];
        assert!(link_span_in_row_at_col(&row, 0, true).is_none());
    }
}
