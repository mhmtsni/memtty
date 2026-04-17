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
        let previous_hovered = self.tab_index_at_position(self.mouse_position);
        let new_hovered = self.tab_index_at_position(position);

        self.mouse_position = position;

        let cursor_changed = self.set_mouse_icon(position);

        if self.dragging_scroll_indicator {
            self.handle_scroll_indicator_drag(position);
            return true;
        }

        // Mouse motion reporting (neovim drag selection için)
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
                    Some(MouseButton::Left) => 32u8, // motion modifier = +32
                    Some(MouseButton::Middle) => 33,
                    Some(MouseButton::Right) => 34,
                    _ => 35, // no button held (AnyEvent)
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

        if previous_hovered == new_hovered && !cursor_changed {
            return false;
        }

        if previous_hovered != new_hovered {
            self.sync_renderer_from_terminal(true);
        }

        true
    }

    pub fn is_position_on_scroll_indicator(&self, position: PhysicalPosition<f64>) -> bool {
        let Some(info) = self.visible_scroll_indicator_info() else {
            return false;
        };

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
                        self.mark_scroll_indicator_interaction();
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

    // fn handle_text_selection(&mut self, position: PhysicalPosition<f64>) {}

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
            self.mark_scroll_indicator_interaction();
        }

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

    pub fn is_mouse_on_tab(
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
