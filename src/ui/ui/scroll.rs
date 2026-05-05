use super::*;

const SCROLL_INDICATOR_FADE_DELAY: Duration = Duration::from_millis(900);
const SCROLL_INDICATOR_FADE_DURATION: Duration = Duration::from_millis(260);
const SCROLL_INDICATOR_FADE_FRAME: Duration = Duration::from_millis(16);
const SCROLL_INDICATOR_INTERACTION_THROTTLE: Duration = Duration::from_millis(90);

impl MyApp {
    pub(super) fn mark_scroll_indicator_interaction(&mut self) {
        self.interaction.scroll_indicator_last_interaction = Some(Instant::now());
        self.interaction.scroll_indicator_last_alpha = 1.0;
    }

    pub(super) fn mark_scroll_indicator_interaction_throttled(&mut self) {
        let now = Instant::now();
        let should_refresh = match self.interaction.scroll_indicator_last_interaction {
            None => true,
            Some(last_interaction) => {
                now.saturating_duration_since(last_interaction)
                    >= SCROLL_INDICATOR_INTERACTION_THROTTLE
                    || self.interaction.scroll_indicator_last_alpha < 0.99
            }
        };

        if should_refresh {
            self.interaction.scroll_indicator_last_interaction = Some(now);
            self.interaction.scroll_indicator_last_alpha = 1.0;
        }
    }

    fn scroll_indicator_alpha_at(&self, now: Instant) -> f32 {
        let Some(last_interaction) = self.interaction.scroll_indicator_last_interaction else {
            return 0.0;
        };

        let elapsed = now.saturating_duration_since(last_interaction);
        if elapsed <= SCROLL_INDICATOR_FADE_DELAY {
            return 1.0;
        }

        let fade_elapsed = elapsed - SCROLL_INDICATOR_FADE_DELAY;
        if fade_elapsed >= SCROLL_INDICATOR_FADE_DURATION {
            return 0.0;
        }

        let progress = fade_elapsed.as_secs_f32() / SCROLL_INDICATOR_FADE_DURATION.as_secs_f32();
        (1.0 - progress).clamp(0.0, 1.0)
    }

    pub(crate) fn update_scroll_indicator_fade(&mut self) -> bool {
        let new_alpha = self.scroll_indicator_alpha_at(Instant::now());

        if (new_alpha - self.interaction.scroll_indicator_last_alpha).abs() < 0.01 {
            return false;
        }

        self.interaction.scroll_indicator_last_alpha = new_alpha;
        self.sync_renderer_from_terminal(false);

        if new_alpha <= 0.0 {
            self.interaction.scroll_indicator_last_interaction = None;
        }

        true
    }

    pub(crate) fn next_scroll_indicator_deadline(&self) -> Option<Instant> {
        let last_interaction = self.interaction.scroll_indicator_last_interaction?;
        let now = Instant::now();
        let fade_start = last_interaction + SCROLL_INDICATOR_FADE_DELAY;
        let fade_end = fade_start + SCROLL_INDICATOR_FADE_DURATION;

        if now < fade_start {
            Some(fade_start)
        } else if now < fade_end {
            Some(now + SCROLL_INDICATOR_FADE_FRAME)
        } else {
            None
        }
    }

    pub(super) fn visible_scroll_indicator_info(&self) -> Option<ScrollIndicatorRenderInfo> {
        let tab = self.session.tabs.get(self.session.active_tab)?;
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
        let scroll_offset = (self.session.scroll_offset as f32).clamp(0.0, max_scroll);

        let position_ratio = 1.0 - (scroll_offset / max_scroll);
        let scrollable_track = (usable_height - indicator_height).max(0.0);
        let position_y = tab_bar_height + scrollable_track * position_ratio;

        let opacity = self.interaction.scroll_indicator_last_alpha.clamp(0.0, 1.0);

        Some(ScrollIndicatorRenderInfo {
            height: indicator_height,
            visible: opacity > 0.0,
            opacity,
            position_y,
            in_alt_screen: tab.terminal.performer.in_alt_screen,
            is_mouse_on_indicator: false, // computed elsewhere
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

        let tab = &mut self.session.tabs[active_tab];

        // Eğer uygulama mouse mode istiyorsa (neovim gibi), PTY'ye gönder
        if tab.terminal.performer.mouse_mode != MouseMode::None {
            let cell_x = ((self.interaction.mouse_position.x - TERMINAL_PADDING_X as f64)
                / self.renderer.cell_width as f64) as usize;
            let cell_y = ((self.interaction.mouse_position.y
                - TAB_HEIGHT as f64
                - TERMINAL_PADDING_Y as f64)
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
        let new_offset = (self.session.scroll_offset + scroll_amount)
            .max(0)
            .min(max_offset);
        if new_offset != self.session.scroll_offset {
            self.session.scroll_offset = new_offset;
            self.mark_scroll_indicator_interaction_throttled();
        }
        self.sync_renderer_from_terminal(false);
    }
}
