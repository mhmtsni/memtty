use glyphon::Color;

use crate::ui::renderer::{
    SettingsControlRenderKind, SettingsItemRenderInfo, SettingsPanelRenderInfo, TAB_HEIGHT,
    TabRenderInfo, UiRect,
};

const ACCENT: Color = Color::rgb(97, 175, 239);
const TAB_BG_ACTIVE: Color = Color::rgb(36, 36, 36);
const TAB_BG_IDLE: Color = Color::rgb(28, 28, 28);
const SETTINGS_BUTTON_BG: Color = Color::rgb(31, 37, 44);
const SETTINGS_BUTTON_BG_HOVER: Color = Color::rgb(50, 61, 72);
const SETTINGS_PANEL_BG: Color = Color::rgb(15, 18, 22);
const SETTINGS_SIDEBAR_BG: Color = Color::rgb(8, 10, 13);
const SETTINGS_PANEL_BORDER: Color = Color::rgb(66, 82, 99);
const SETTINGS_ITEM_BG: Color = Color::rgb(24, 30, 37);
const SETTINGS_ITEM_BG_HOVER: Color = Color::rgb(34, 43, 53);
const SETTINGS_CONTROL_BG: Color = Color::rgb(6, 9, 12);
const SETTINGS_CONTROL_HOVER: Color = Color::rgb(53, 66, 82);
const SETTINGS_TOGGLE_ON: Color = Color::rgb(70, 168, 124);
const SETTINGS_WARNING: Color = Color::rgb(205, 113, 86);

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

pub(super) fn render_tab_overlay(
    renderer: &mut super::Renderer,
    tabs: Option<Vec<TabRenderInfo>>,
    settings: Option<SettingsPanelRenderInfo>,
) {
    let Some(tabs) = tabs else {
        return;
    };
    render_tabs(renderer, &tabs);

    if let Some(settings) = settings {
        render_settings_button(renderer, &settings);
        render_settings_panel(renderer, settings);
    }
}

fn render_tabs(renderer: &mut super::Renderer, tabs: &[TabRenderInfo]) {
    let active_id = tabs.iter().position(|t| t.active);

    for (i, tab) in tabs.iter().enumerate() {
        let is_active = tab.active;
        let is_hovered = tab.is_hovered;
        let bg_color = if is_active {
            TAB_BG_ACTIVE
        } else {
            TAB_BG_IDLE
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
                ACCENT,
                divider_alpha(i, is_active, active_id),
            );
        }

        if is_active {
            renderer.push_rect_pixels(
                tab.x as f32,
                tab.y as f32 + TAB_HEIGHT as f32 - 2.0,
                tab.width as f32,
                2.0,
                ACCENT,
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
                ACCENT,
                0.2,
            );
        }
    }
}

fn render_settings_button(renderer: &mut super::Renderer, settings: &SettingsPanelRenderInfo) {
    let button_bg = if settings.button_hovered {
        SETTINGS_BUTTON_BG_HOVER
    } else if settings.is_open {
        Color::rgb(45, 63, 82)
    } else {
        SETTINGS_BUTTON_BG
    };
    renderer.push_rect_pixels(
        settings.button_rect.x as f32,
        settings.button_rect.y as f32,
        settings.button_rect.width as f32,
        settings.button_rect.height as f32,
        button_bg,
        1.0,
    );

    renderer.push_rect_pixels(
        settings.button_rect.x as f32,
        settings.button_rect.y as f32,
        settings.button_rect.width as f32,
        1.0,
        SETTINGS_PANEL_BORDER,
        0.75,
    );

    render_settings_icon(
        renderer,
        settings.button_rect,
        settings.button_hovered || settings.is_open,
    );
}

fn render_settings_panel(renderer: &mut super::Renderer, settings: SettingsPanelRenderInfo) {
    if !settings.is_open {
        return;
    }

    renderer.push_rect_pixels(
        settings.panel_rect.x as f32 + 8.0,
        settings.panel_rect.y as f32 + 10.0,
        settings.panel_rect.width as f32,
        settings.panel_rect.height as f32,
        Color::rgb(0, 0, 0),
        1.0,
    );

    renderer.push_rect_pixels(
        settings.panel_rect.x as f32,
        settings.panel_rect.y as f32,
        settings.panel_rect.width as f32,
        settings.panel_rect.height as f32,
        SETTINGS_PANEL_BG,
        1.0,
    );

    renderer.push_rect_pixels(
        settings.panel_rect.x as f32,
        settings.panel_rect.y as f32,
        settings.panel_rect.width as f32,
        58.0,
        Color::rgb(17, 21, 26),
        1.0,
    );

    renderer.push_rect_pixels(
        settings.sidebar_rect.x as f32,
        settings.sidebar_rect.y as f32,
        settings.sidebar_rect.width as f32,
        settings.sidebar_rect.height as f32,
        SETTINGS_SIDEBAR_BG,
        1.0,
    );

    renderer.push_rect_pixels(
        settings.panel_rect.x as f32,
        settings.panel_rect.y as f32,
        settings.panel_rect.width as f32,
        1.0,
        SETTINGS_PANEL_BORDER,
        1.0,
    );

    renderer.push_rect_pixels(
        settings.sidebar_rect.x as f32 + settings.sidebar_rect.width as f32 - 1.0,
        settings.sidebar_rect.y as f32,
        1.0,
        settings.sidebar_rect.height as f32,
        SETTINGS_PANEL_BORDER,
        1.0,
    );

    renderer.push_rect_pixels(
        settings.content_rect.x as f32,
        settings.content_rect.y as f32 + 60.0,
        settings.content_rect.width as f32,
        1.0,
        SETTINGS_PANEL_BORDER,
        1.0,
    );

    renderer.push_rect_pixels(
        settings.sidebar_rect.x as f32 + 10.0,
        settings.sidebar_rect.y as f32 + 67.0,
        settings.sidebar_rect.width as f32 - 20.0,
        34.0,
        Color::rgb(45, 63, 82),
        1.0,
    );

    for item in &settings.items {
        let item_bg = if item.is_hovered {
            SETTINGS_ITEM_BG_HOVER
        } else {
            SETTINGS_ITEM_BG
        };
        renderer.push_rect_pixels(
            item.rect.x as f32,
            item.rect.y as f32,
            item.rect.width as f32,
            item.rect.height as f32,
            item_bg,
            1.0,
        );

        renderer.push_rect_pixels(
            item.rect.x as f32,
            item.rect.y as f32,
            3.0,
            item.rect.height as f32,
            if item.is_hovered {
                ACCENT
            } else {
                Color::rgb(45, 55, 66)
            },
            1.0,
        );

        render_settings_control(renderer, item);
    }
}

fn render_settings_icon(renderer: &mut super::Renderer, rect: UiRect, active: bool) {
    let color = if active {
        Color::rgb(235, 242, 249)
    } else {
        Color::rgb(166, 178, 190)
    };
    let x = rect.x as f32 + 12.0;
    let y = rect.y as f32 + 11.0;
    for (line, knob) in [(0.0, 12.0), (8.0, 4.0), (16.0, 16.0)] {
        renderer.push_rect_pixels(x, y + line, 20.0, 2.0, color, 1.0);
        renderer.push_rect_pixels(x + knob, y + line - 3.0, 4.0, 8.0, color, 1.0);
    }
}

fn render_settings_control(renderer: &mut super::Renderer, item: &SettingsItemRenderInfo) {
    match item.control {
        SettingsControlRenderKind::Menu => {
            render_rect(renderer, item.primary_rect, SETTINGS_CONTROL_BG, 1.0);
            render_caret(renderer, item.primary_rect);
        }
        SettingsControlRenderKind::Toggle { enabled } => {
            let bg = if enabled {
                SETTINGS_TOGGLE_ON
            } else {
                SETTINGS_CONTROL_BG
            };
            render_rect(renderer, item.primary_rect, bg, 1.0);
            let knob_size = item.primary_rect.height.saturating_sub(10);
            let knob_x = if enabled {
                item.primary_rect.x + item.primary_rect.width.saturating_sub(knob_size + 5)
            } else {
                item.primary_rect.x + 5
            };
            render_rect(
                renderer,
                UiRect {
                    x: knob_x,
                    y: item.primary_rect.y + 5,
                    width: knob_size,
                    height: knob_size,
                },
                Color::rgb(244, 247, 250),
                1.0,
            );
        }
        SettingsControlRenderKind::Stepper => {
            render_rect(renderer, item.primary_rect, SETTINGS_CONTROL_BG, 1.0);
            let left = UiRect {
                x: item.primary_rect.x,
                y: item.primary_rect.y,
                width: item.primary_rect.height,
                height: item.primary_rect.height,
            };
            let right = item.secondary_rect.unwrap_or(UiRect {
                x: item.primary_rect.x
                    + item
                        .primary_rect
                        .width
                        .saturating_sub(item.primary_rect.height),
                y: item.primary_rect.y,
                width: item.primary_rect.height,
                height: item.primary_rect.height,
            });
            render_rect(renderer, left, SETTINGS_CONTROL_HOVER, 0.75);
            render_rect(renderer, right, SETTINGS_CONTROL_HOVER, 0.75);
            render_minus(renderer, left);
            render_plus(renderer, right);
        }
        SettingsControlRenderKind::Button => {
            render_rect(renderer, item.primary_rect, SETTINGS_WARNING, 0.92);
        }
    }
}

fn render_rect(renderer: &mut super::Renderer, rect: UiRect, color: Color, alpha: f32) {
    renderer.push_rect_pixels(
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
        color,
        alpha,
    );
}

fn render_caret(renderer: &mut super::Renderer, rect: UiRect) {
    let x = rect.x as f32 + rect.width as f32 - 18.0;
    let y = rect.y as f32 + rect.height as f32 * 0.5 - 1.0;
    renderer.push_rect_pixels(x, y, 8.0, 2.0, Color::rgb(184, 196, 210), 1.0);
    renderer.push_rect_pixels(x + 2.0, y + 3.0, 4.0, 2.0, Color::rgb(184, 196, 210), 1.0);
}

fn render_minus(renderer: &mut super::Renderer, rect: UiRect) {
    renderer.push_rect_pixels(
        rect.x as f32 + 13.0,
        rect.y as f32 + rect.height as f32 * 0.5,
        rect.width as f32 - 26.0,
        2.0,
        Color::rgb(232, 238, 245),
        1.0,
    );
}

fn render_plus(renderer: &mut super::Renderer, rect: UiRect) {
    render_minus(renderer, rect);
    renderer.push_rect_pixels(
        rect.x as f32 + rect.width as f32 * 0.5 - 1.0,
        rect.y as f32 + 11.0,
        2.0,
        rect.height as f32 - 22.0,
        Color::rgb(232, 238, 245),
        1.0,
    );
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
