use super::*;
use crate::terminal::Cell;

impl MyApp {
    pub fn sync_renderer_from_terminal(&mut self, content_changed: bool) {
        let tabs = self.visible_tab_info(self.session.tabs.len());
        let settings = Some(self.settings_panel_info());
        let Some(active_tab) = self.normalize_active_tab() else {
            self.renderer
                .set_cells(&[], None, tabs, settings, None, content_changed);
            return;
        };

        let decorated_rows = self.build_decorated_visible_rows(active_tab);
        let row_refs: Vec<&Vec<Cell>> = decorated_rows.iter().collect();

        let cursor = self.visible_cursor_info(self.renderer.visible_row_capacity(), row_refs.len());
        let scroll_indicator = self.visible_scroll_indicator_info();
        self.window.set_cursor(self.interaction.mouse_icon);

        self.renderer.set_cells(
            &row_refs,
            cursor,
            tabs,
            settings,
            scroll_indicator,
            content_changed,
        );
    }

    pub(super) fn visible_tab_info(&self, tab_count: usize) -> Option<Vec<TabRenderInfo>> {
        if tab_count == 0 || self.session.active_tab >= tab_count {
            return None;
        }

        let tab_index = self.session.active_tab;
        let tab_id = self.session.tabs.get(tab_index)?.id;

        let tab_width = self.renderer.width as f32 / tab_count as f32;

        self.session
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let base_title = if tab.terminal.performer.title.is_empty() {
                    "~"
                } else {
                    tab.terminal.performer.title.as_str()
                };
                let title = format!("⌘ + {}  {}", i + 1, base_title);

                TabRenderInfo {
                    title,
                    is_hovered: (self.interaction.mouse_position.x >= i as f64 * tab_width as f64)
                        && (self.interaction.mouse_position.x
                            < (i as f64 + 1.0) * tab_width as f64)
                        && (self.interaction.mouse_position.y >= 0.0)
                        && (self.interaction.mouse_position.y < TAB_HEIGHT as f64),
                    x: (i as f32 * tab_width).round() as usize,
                    y: 0,
                    width: tab_width.round() as usize,
                    height: TAB_HEIGHT,
                    active: tab.id == tab_id,
                }
            })
            .collect::<Vec<_>>()
            .into()
    }

    pub(super) fn visible_cursor_info(
        &self,
        requested_visible_rows: usize,
        actual_visible_rows: usize,
    ) -> Option<CursorRenderInfo> {
        let tab = self.active_tab()?;

        if !self.cursor_render_visible() {
            return None;
        }

        let Some(window) = tab
            .terminal
            .visible_row_window(self.session.scroll_offset, requested_visible_rows)
        else {
            return None;
        };

        let scrollback_len = window.scrollback_len;
        let cursor_abs_row = scrollback_len + tab.terminal.performer.cursor_y;
        if cursor_abs_row < window.start || cursor_abs_row >= window.end {
            return None;
        }

        let cursor_row = cursor_abs_row - window.start;
        if cursor_row >= actual_visible_rows {
            return None;
        }

        let cursor_style = if self.has_focus {
            match tab.terminal.performer.cursor_style {
                CursorStyle::Block => CursorRenderStyle::Block,
                CursorStyle::Underline => CursorRenderStyle::Underline,
                CursorStyle::Bar => CursorRenderStyle::Bar,
            }
        } else {
            CursorRenderStyle::Unfocused
        };

        let blink_on = if self.has_focus {
            !self.cursor_blink_active() || self.cursor_blink_on
        } else {
            true
        };

        Some(CursorRenderInfo {
            col: tab.terminal.performer.cursor_x,
            row: cursor_row,
            style: cursor_style,
            color: CURSOR_COLOR,
            blink_on,
        })
    }
}
