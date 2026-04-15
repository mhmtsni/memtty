use glyphon::Color;

use crate::ui::renderer::{INDICATOR_WIDTH, ScrollIndicatorRenderInfo, TERMINAL_PADDING_X};

pub(super) fn render_scroll_indicator_overlay(
    renderer: &mut super::Renderer,
    scroll_indicator: Option<ScrollIndicatorRenderInfo>,
) {
    let Some(scroll_indicator) = scroll_indicator else {
        return;
    };
    if scroll_indicator.visible {
        renderer.push_rect_pixels(
            renderer.width as f32 - INDICATOR_WIDTH - TERMINAL_PADDING_X as f32,
            scroll_indicator.position_y as f32,
            INDICATOR_WIDTH,
            scroll_indicator.height,
            Color::rgb(255, 255, 255),
            1.0,
        );
    }
}
