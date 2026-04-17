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

        let tab = &mut self.tabs[active_tab];

        // Eğer uygulama mouse mode istiyorsa (neovim gibi), PTY'ye gönder
        if tab.terminal.performer.mouse_mode != MouseMode::None {
            let cell_x = ((self.mouse_position.x - TERMINAL_PADDING_X as f64)
                / self.renderer.cell_width as f64) as usize;
            let cell_y = ((self.mouse_position.y - TAB_HEIGHT as f64 - TERMINAL_PADDING_Y as f64)
                / self.renderer.line_height as f64) as usize;

            // Yukarı scroll = button 64, aşağı scroll = button 65
            let btn = if scroll_amount > 0 { 64u8 } else { 65u8 };
            let steps = scroll_amount.unsigned_abs() as usize;

            for _ in 0..steps {
                tab.terminal
                    .performer
                    .report_mouse(cell_x, cell_y, btn, true);
            }

            if let Some(tx) = &tab.tx {
                for reply in tab.terminal.performer.drain_pty_replies() {
                    let _ = tx.send(PtyInput::Data(reply));
                }
            }
            return; // scrollback'e düşme
        }

        // Normal terminal: scrollback
        let max_offset = tab.terminal.performer.scrollback.len() as i32;
        self.scroll_offset = (self.scroll_offset + scroll_amount).max(0).min(max_offset);
        self.sync_renderer_from_terminal(false);
    }
}
