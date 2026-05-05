use super::*;

impl MyApp {
    pub fn handle_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(active_tab) = self.normalize_active_tab() else {
            self.renderer.resize(size.width, size.height);
            return;
        };

        let (new_cols, new_rows) = self.terminal_grid_size_for_pixels(size.width, size.height);

        self.resize_all_tabs(new_cols, new_rows);

        self.renderer.resize(size.width, size.height);

        let max_offset = self.session.tabs[active_tab]
            .terminal
            .performer
            .scrollback
            .len() as i32;
        self.session.scroll_offset = self.session.scroll_offset.min(max_offset).max(0);
        self.sync_renderer_from_terminal(true);
    }

    pub(super) fn refit_terminal_to_renderer(&mut self) {
        let Some(active_tab) = self.normalize_active_tab() else {
            return;
        };

        let (new_cols, new_rows) =
            self.terminal_grid_size_for_pixels(self.renderer.width, self.renderer.height);

        self.resize_all_tabs(new_cols, new_rows);

        let max_offset = self.session.tabs[active_tab]
            .terminal
            .performer
            .scrollback
            .len() as i32;
        self.session.scroll_offset = self.session.scroll_offset.min(max_offset).max(0);
        self.sync_renderer_from_terminal(true);
    }

    fn terminal_grid_size_for_pixels(&self, width: u32, height: u32) -> (u16, u16) {
        let (cell_width, line_height) = self.renderer.cell_size();
        let content_width = (width as f32 - 2.0 * TERMINAL_PADDING_X).max(0.0);
        let content_height =
            (height as f32 - TAB_HEIGHT as f32 - 2.0 * TERMINAL_PADDING_Y).max(0.0);
        let new_cols = (content_width / cell_width).floor().max(10.0) as u16;
        let new_rows = (content_height / line_height).floor().max(5.0) as u16;

        (new_cols, new_rows)
    }
}
