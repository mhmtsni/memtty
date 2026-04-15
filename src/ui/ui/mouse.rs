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
}
