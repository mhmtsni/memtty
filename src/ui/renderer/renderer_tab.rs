use glyphon::Color;

use crate::ui::renderer::{TAB_HEIGHT, TabRenderInfo};

fn divider_alpha(i: usize, is_active: bool, active_index: Option<usize>) -> f32 {
    if i == 0 {
        return 0.0;
    }
    if is_active {
        return 1.0;
    }
    if active_index.is_some_and(|ai| i == ai + 1) {
        return 1.0;
    }
    0.2
}

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
                Color::rgb(97, 175, 239),
                divider_alpha(i, is_active, active_id),
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
        } else if is_hovered {
            renderer.push_rect_pixels(
                tab.x as f32,
                tab.y as f32,
                tab.width as f32,
                tab.height as f32,
                Color::rgb(255, 255, 255),
                0.1,
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

#[cfg(test)]
mod tests {
    use super::divider_alpha;

    #[test]
    fn divider_alpha_is_dim_by_default() {
        assert!((divider_alpha(1, false, Some(10)) - 0.2).abs() < f32::EPSILON);
        assert!((divider_alpha(3, false, None) - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn divider_alpha_is_bright_for_active_tab_divider() {
        assert!((divider_alpha(2, true, Some(0)) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn divider_alpha_is_bright_for_divider_right_of_active_tab() {
        assert!((divider_alpha(2, false, Some(1)) - 1.0).abs() < f32::EPSILON);
        assert!((divider_alpha(1, false, Some(0)) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn divider_alpha_is_never_drawn_for_first_divider() {
        assert!((divider_alpha(0, false, Some(0)) - 0.0).abs() < f32::EPSILON);
        assert!((divider_alpha(0, true, Some(0)) - 0.0).abs() < f32::EPSILON);
    }
}
