use super::*;

impl MyApp {
    pub fn update_cursor_blink(&mut self) -> bool {
        let blink_interval = Duration::from_millis(500);
        let now = Instant::now();

        let cursor_visible = self.cursor_render_visible();

        if self.cursor_blink_active()
            && cursor_visible
            && now.duration_since(self.last_blink) >= blink_interval
        {
            self.cursor_blink_on = !self.cursor_blink_on;
            self.last_blink = now;
            self.sync_renderer_from_terminal(false);
            return true;
        }
        false
    }

    pub fn next_blink_deadline(&self) -> Option<Instant> {
        let cursor_visible = self.cursor_render_visible();

        if self.cursor_blink_active() && cursor_visible {
            Some(self.last_blink + Duration::from_millis(500))
        } else {
            None
        }
    }

    pub fn update_has_focus(&mut self, has_focus: bool) {
        if self.has_focus == has_focus {
            return;
        }

        let focus_reporting_enabled = self
            .session
            .tabs
            .get(self.session.active_tab)
            .map(|tab| tab.terminal.performer.focus_reporting_enabled())
            .unwrap_or(false);
        self.has_focus = has_focus;

        // Avoid resuming focused blinking in the "off" phase.
        self.cursor_blink_on = true;

        if focus_reporting_enabled {
            let sequence = if has_focus {
                b"\x1b[I".to_vec()
            } else {
                b"\x1b[O".to_vec()
            };
            self.send_to_pty(PtyInput::Data(sequence));
        }

        self.last_blink = Instant::now();
        self.sync_renderer_from_terminal(true);
    }

    pub(super) fn create_new_tab(&mut self, proxy: EventLoopProxy<Message>) {
        let tab_id = self.next_tab_id();

        let tx = spawn_pty_for_tab(tab_id, proxy);

        self.session.tabs.push(Tab {
            id: tab_id,
            terminal: Terminal::new(),
            tx: Some(tx),
            pending_pty: Vec::new(),
            pending_pty_offset: 0,
            input_line: String::new(),
            history_completion: None,
            history_preview: None,
            shell_history: Vec::new(),
        });

        self.session.active_tab = self.session.tabs.len() - 1;
        self.clear_selection();
        self.handle_resize(self.window.inner_size());
        self.sync_renderer_from_terminal(true);
    }
}
