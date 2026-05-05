use super::*;

impl MyApp {
    pub(super) fn normalize_active_tab(&mut self) -> Option<usize> {
        if self.session.tabs.is_empty() {
            return None;
        }

        if self.session.active_tab >= self.session.tabs.len() {
            self.session.active_tab = self.session.tabs.len() - 1;
        }

        Some(self.session.active_tab)
    }

    pub(super) fn next_tab_id(&self) -> usize {
        self.session
            .tabs
            .iter()
            .map(|t| t.id)
            .max()
            .map(|id| id + 1)
            .unwrap_or(0)
    }

    pub(super) fn close_active_tab(&mut self) -> bool {
        if self.session.tabs.len() <= 1 {
            return false;
        }

        let Some(active_tab) = self.normalize_active_tab() else {
            return false;
        };

        let mut removed = self.session.tabs.remove(active_tab);
        if let Some(tx) = removed.tx.take() {
            let _ = tx.send(PtyInput::Shutdown);
        }

        if self.session.active_tab >= self.session.tabs.len() {
            self.session.active_tab = self.session.tabs.len() - 1;
        }

        self.clear_selection();
        self.reset_scrollback_view();
        self.sync_renderer_from_terminal(true);
        true
    }

    pub(super) fn resize_all_tabs(&mut self, new_cols: u16, new_rows: u16) {
        let cols = new_cols as usize;
        let rows = new_rows as usize;

        for tab in &mut self.session.tabs {
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
        if let Some(tx) = self.active_tab().and_then(|tab| tab.tx.as_ref()) {
            let _ = tx.send(data);
        }
    }

    pub(crate) fn queue_pty_data(&mut self, tab_id: usize, data: &[u8]) {
        if let Some(tab) = self.session.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.pending_pty.extend_from_slice(data);
        }
    }

    pub(crate) fn has_pending_pty_data(&self) -> bool {
        self.session
            .tabs
            .iter()
            .any(|tab| tab.pending_pty_offset < tab.pending_pty.len())
    }

    pub(crate) fn process_pending_pty_data(&mut self, byte_budget: usize) -> bool {
        const PARSE_CHUNK_BYTES: usize = 16 * 1024;
        const COMPACT_THRESHOLD: usize = 64 * 1024;

        let mut remaining = byte_budget;
        let mut any_processed = false;

        for tab in &mut self.session.tabs {
            while remaining > 0 {
                let available = tab.pending_pty.len().saturating_sub(tab.pending_pty_offset);
                if available == 0 {
                    break;
                }

                let take = available.min(remaining).min(PARSE_CHUNK_BYTES);
                let start = tab.pending_pty_offset;
                let end = start + take;

                let replies = tab.terminal.process(&tab.pending_pty[start..end]);
                tab.pending_pty_offset = end;
                remaining -= take;
                any_processed = true;

                if tab.pending_pty_offset == tab.pending_pty.len() {
                    tab.pending_pty.clear();
                    tab.pending_pty_offset = 0;
                } else if tab.pending_pty_offset >= COMPACT_THRESHOLD {
                    tab.pending_pty.drain(..tab.pending_pty_offset);
                    tab.pending_pty_offset = 0;
                }

                if let Some(tx) = &tab.tx {
                    for reply in replies {
                        let _ = tx.send(PtyInput::Data(reply));
                    }
                }
            }

            if remaining == 0 {
                break;
            }
        }

        any_processed
    }
}
