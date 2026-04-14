use glyphon::Color;

use crate::ui::renderer::{TAB_HEIGHT, TabRenderInfo};

pub(super) fn render_tab_overlay(renderer: &mut super::Renderer, tabs: Option<Vec<TabRenderInfo>>) {
    let Some(tabs) = tabs else {
        return;
    };
    let active_id = tabs.iter().position(|t| t.active);

    tabs.iter().enumerate().for_each(|(i, tab)| {
        let is_active = tab.active;
        let is_hovered = tab.is_hovered;
        let bg_color = if is_active {
            Color::rgb(36, 36, 36)
        } else {
            Color::rgb(28, 28, 28)
        };
        renderer.push_rect_pixels(
            tab.x as f32,
            tab.y as f32,
            tab.width as f32,
            tab.height as f32,
            bg_color,
            1.0,
        );
        if i != 0 {
            renderer.push_rect_pixels(
                tab.x as f32,
                tab.y as f32,
                2.0,
                tab.height as f32,
                Color::rgb(97, 175, 239), // accent line
                (is_active || i == active_id.unwrap() + 1)
                    .then_some(1.0)
                    .unwrap_or(0.2),
            );
        }

        if is_hovered {
            renderer.push_rect_pixels(
                tab.x as f32,
                tab.y as f32,
                tab.width as f32,
                tab.height as f32,
                Color::rgb(255, 255, 255), // hover overlay
                0.1,
            );
        }

        if is_active {
            renderer.push_rect_pixels(
                tab.x as f32,
                tab.y as f32 + TAB_HEIGHT as f32 - 2.0,
                tab.width as f32,
                2.0,
                Color::rgb(97, 175, 239), // accent line
                1.0,
            );
        } else {
            renderer.push_rect_pixels(
                tab.x as f32,
                tab.y as f32 + TAB_HEIGHT as f32 - 2.0,
                tab.width as f32,
                2.0,
                Color::rgb(97, 175, 239), // accent line
                0.2,
            );
        }
    });
}
