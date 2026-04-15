use super::*;

impl MyApp {
    pub(super) fn normalize_active_tab(&mut self) -> Option<usize> {
        if self.tabs.is_empty() {
            return None;
        }

        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }

        Some(self.active_tab)
    }

    pub(super) fn cursor_blink_active(&self) -> bool {
        if !self.has_focus {
            return false;
        }

        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.terminal.performer.cursor_blinking)
            .unwrap_or(false)
    }

    pub(super) fn next_tab_id(&self) -> usize {
        self.tabs
            .iter()
            .map(|t| t.id)
            .max()
            .map(|id| id + 1)
            .unwrap_or(0)
    }

    pub(super) fn close_active_tab(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }

        let Some(active_tab) = self.normalize_active_tab() else {
            return false;
        };

        let mut removed = self.tabs.remove(active_tab);
        if let Some(tx) = removed.tx.take() {
            let _ = tx.send(PtyInput::Shutdown);
        }

        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }

        self.reset_scrollback_view();
        self.sync_renderer_from_terminal(true);
        true
    }

    pub(super) fn cursor_render_visible(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .map(|tab| tab.terminal.performer.cursor_visible)
            .unwrap_or(false)
            && self.scroll_offset == 0
    }

    pub(super) fn resize_all_tabs(&mut self, new_cols: u16, new_rows: u16) {
        let cols = new_cols as usize;
        let rows = new_rows as usize;

        for tab in &mut self.tabs {
            tab.terminal.performer.resize(cols, rows);
            if let Some(tx) = &tab.tx {
                let _ = tx.send(PtyInput::Resize {
                    cols: new_cols,
                    rows: new_rows,
                });
            }
        }
    }

    pub(super) fn send_to_pty(&mut self, data: PtyInput) {
        if self.tabs.is_empty() {
            return;
        }

        let active_tab = self.active_tab.min(self.tabs.len() - 1);
        if let Some(tx) = &self.tabs[active_tab].tx {
            let _ = tx.send(data);
        }
    }

    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    pub(super) fn reset_scrollback_view(&mut self) {
        self.scroll_offset = 0;
    }
}
