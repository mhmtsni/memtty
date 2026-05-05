use super::*;

impl MyApp {
    pub(super) fn active_tab(&self) -> Option<&Tab> {
        self.session.tabs.get(self.session.active_tab)
    }

    pub(super) fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.session.tabs.get_mut(self.session.active_tab)
    }

    pub(super) fn cursor_blink_active(&self) -> bool {
        if !self.has_focus {
            return false;
        }

        self.active_tab()
            .map(|tab| tab.terminal.performer.cursor_blinking)
            .unwrap_or(false)
    }

    pub(super) fn cursor_render_visible(&self) -> bool {
        self.active_tab()
            .map(|tab| tab.terminal.performer.cursor_visible)
            .unwrap_or(false)
            && self.session.scroll_offset == 0
    }

    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }
}
