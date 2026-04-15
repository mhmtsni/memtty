use super::*;

impl MyApp {
    pub(super) fn visible_scroll_indicator_info(&self) -> Option<ScrollIndicatorRenderInfo> {
        let tab = self.tabs.get(self.active_tab)?;
        let scrollback_len = tab.terminal.performer.scrollback.len() as f32;
        let visible_lines = tab.terminal.performer.rows as f32;
        let total_lines = scrollback_len + visible_lines;

        if total_lines <= 0.0 || scrollback_len == 0.0 {
            return None;
        }

        let viewport_height = self.renderer.height as f32;
        let tab_bar_height = TAB_HEIGHT as f32;
        let usable_height = viewport_height - tab_bar_height;

        let raw_height = (visible_lines / total_lines) * usable_height;
        let indicator_height = raw_height.max(16.0).min(usable_height);

        let max_scroll = scrollback_len;
        let scroll_offset = (self.scroll_offset as f32).clamp(0.0, max_scroll);

        let position_ratio = 1.0 - (scroll_offset / max_scroll);
        let scrollable_track = (usable_height - indicator_height).max(0.0);
        let position_y = tab_bar_height + scrollable_track * position_ratio;

        Some(ScrollIndicatorRenderInfo {
            height: indicator_height,
            visible: true,
            position_y,
        })
    }

    pub fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let Some(active_tab) = self.normalize_active_tab() else {
            return;
        };

        let scroll_amount = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as i32,
            MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
        };

        if scroll_amount == 0 {
            return;
        }

        let max_offset = self.tabs[active_tab].terminal.performer.scrollback.len() as i32;
        self.scroll_offset = (self.scroll_offset + scroll_amount).max(0).min(max_offset);
        self.sync_renderer_from_terminal(false);
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> bool {
        let previous_hovered = self.tab_index_at_position(self.mouse_position);
        let new_hovered = self.tab_index_at_position(position);

        self.mouse_position = position;

        if self.dragging_scroll_indicator {
            self.handle_scroll_indicator_drag(position);
            return true;
        }

        if previous_hovered == new_hovered {
            return false;
        }

        self.sync_renderer_from_terminal(true);
        true
    }

    fn is_position_on_scroll_indicator(&self, position: PhysicalPosition<f64>) -> bool {
        let Some(info) = self.visible_scroll_indicator_info() else {
            return false;
        };

        if !info.visible {
            return false;
        }

        let indicator_x =
            self.renderer.width as f64 - INDICATOR_WIDTH as f64 - TERMINAL_PADDING_X as f64;
        let indicator_y = info.position_y as f64;
        let indicator_w = INDICATOR_WIDTH as f64;
        let indicator_h = info.height as f64;

        position.x >= indicator_x
            && position.x <= indicator_x + indicator_w
            && position.y >= indicator_y
            && position.y <= indicator_y + indicator_h
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
                    } else {
                        self.handle_tab_click(self.mouse_position);
                    }
                }
            }
            ElementState::Released => {
                self.mouse_button_held = None;
                self.mouse_hold_start = None;
                self.dragging_scroll_indicator = false;
            }
        }
        self.sync_renderer_from_terminal(true);
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
        self.scroll_offset = (self.drag_start_scroll_offset + offset_delta)
            .max(0)
            .min(max_offset);

        self.sync_renderer_from_terminal(false);
    }

    fn handle_tab_click(&mut self, position: PhysicalPosition<f64>) {
        let Some(mut tabs) = self.visible_tab_info(self.tabs.len()) else {
            return;
        };

        for (index, tab) in tabs.iter_mut().enumerate() {
            if self.is_mouse_on_tab(position, tab) {
                self.active_tab = index;
                self.reset_scrollback_view();
                self.sync_renderer_from_terminal(true);
                return;
            }
        }
    }

    fn is_mouse_on_tab(&self, position: PhysicalPosition<f64>, tab: &mut TabRenderInfo) -> bool {
        position.x >= tab.x as f64
            && position.x < (tab.x + tab.width) as f64
            && position.y >= tab.y as f64
            && position.y < (tab.y + tab.height) as f64
    }

    fn tab_index_at_position(&self, position: PhysicalPosition<f64>) -> Option<usize> {
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

