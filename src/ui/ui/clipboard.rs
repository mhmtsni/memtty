use super::*;

impl MyApp {
    pub(super) fn handle_paste(&mut self) {
        let Ok(mut clipboard) = Clipboard::new() else {
            return;
        };

        let Ok(text) = clipboard.get_text() else {
            return;
        };

        if text.is_empty() {
            return;
        }

        let normalized = text.replace("\r\n", "\n").replace('\n', "\r");

        let bracketed_paste_enabled = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.terminal.performer.bracketed_paste)
            .unwrap_or(false);

        let data = if bracketed_paste_enabled {
            let mut data = Vec::with_capacity(normalized.len() + 12);
            data.extend_from_slice(b"\x1b[200~");
            data.extend_from_slice(normalized.as_bytes());
            data.extend_from_slice(b"\x1b[201~");
            data
        } else {
            normalized.into_bytes()
        };

        self.send_to_pty(PtyInput::Data(data));
        self.reset_scrollback_view();
    }
}
