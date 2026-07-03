use super::*;

const SETTINGS_BUTTON_WIDTH: usize = 44;
const SETTINGS_BUTTON_HEIGHT: usize = 40;
const SETTINGS_RIGHT_MARGIN: usize = 14;
const PANEL_WIDTH: usize = 820;
const PANEL_HEIGHT: usize = 500;
const SIDEBAR_WIDTH: usize = 188;
const CONTENT_PADDING: usize = 28;
const ROW_HEIGHT: usize = 72;
const ROW_GAP: usize = 11;
const CONTROL_WIDTH: usize = 168;
const CONTROL_HEIGHT: usize = 34;
const STEPPER_BUTTON_WIDTH: usize = 42;

#[derive(Clone, Copy)]
enum AppearanceSettingKey {
    FontFamily,
    Ligatures,
    LineHeight,
    FontSize,
    ResetAppearance,
}

impl AppearanceSettingKey {
    fn all() -> [Self; 5] {
        [
            Self::FontFamily,
            Self::Ligatures,
            Self::LineHeight,
            Self::FontSize,
            Self::ResetAppearance,
        ]
    }

    fn title(self) -> &'static str {
        match self {
            Self::FontFamily => "Font family",
            Self::Ligatures => "Ligatures",
            Self::LineHeight => "Line height",
            Self::FontSize => "Font size",
            Self::ResetAppearance => "Reset",
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::FontFamily => "Cycle through installed mono fonts",
            Self::Ligatures => "Use advanced shaping for code glyphs",
            Self::LineHeight => "Adjust vertical density",
            Self::FontSize => "Resize terminal text",
            Self::ResetAppearance => "Restore startup typography",
        }
    }

    fn value(self, app: &MyApp) -> String {
        match self {
            Self::FontFamily => truncate_value(app.renderer.current_font_family_label(), 18),
            Self::Ligatures => {
                if app.renderer.ligatures_enabled() {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            Self::LineHeight => format!("{:.2}x", app.renderer.line_height_factor()),
            Self::FontSize => format!("{:.0}px", app.renderer.font_size),
            Self::ResetAppearance => "Reset".to_string(),
        }
    }

    fn control(self, app: &MyApp) -> SettingsControlRenderKind {
        match self {
            Self::FontFamily => SettingsControlRenderKind::Menu,
            Self::Ligatures => SettingsControlRenderKind::Toggle {
                enabled: app.renderer.ligatures_enabled(),
            },
            Self::LineHeight | Self::FontSize => SettingsControlRenderKind::Stepper,
            Self::ResetAppearance => SettingsControlRenderKind::Button,
        }
    }
}

fn truncate_value(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        value.to_string()
    }
}

#[derive(Clone, Copy)]
enum SettingsClickTarget {
    Row(AppearanceSettingKey),
    Primary(AppearanceSettingKey),
    Secondary(AppearanceSettingKey),
}

impl MyApp {
    pub(super) fn toggle_settings_panel(&mut self) {
        self.interaction.settings_panel_open = !self.interaction.settings_panel_open;
    }

    pub(super) fn close_settings_panel(&mut self) {
        self.interaction.settings_panel_open = false;
    }

    fn settings_button_rect(&self) -> crate::ui::renderer::UiRect {
        let x = self
            .renderer
            .width
            .saturating_sub((SETTINGS_BUTTON_WIDTH + SETTINGS_RIGHT_MARGIN) as u32)
            as usize;
        let y = (TAB_HEIGHT.saturating_sub(SETTINGS_BUTTON_HEIGHT)) / 2;
        crate::ui::renderer::UiRect {
            x,
            y,
            width: SETTINGS_BUTTON_WIDTH,
            height: SETTINGS_BUTTON_HEIGHT,
        }
    }

    fn settings_panel_rect(&self) -> crate::ui::renderer::UiRect {
        let width = PANEL_WIDTH
            .min(self.renderer.width.saturating_sub(32) as usize)
            .max(360);
        let height = PANEL_HEIGHT
            .min(self.renderer.height.saturating_sub(TAB_HEIGHT as u32 + 24) as usize)
            .max(320);
        let x = ((self.renderer.width as usize).saturating_sub(width)) / 2;
        let y = TAB_HEIGHT
            + (((self.renderer.height as usize).saturating_sub(TAB_HEIGHT + height)) / 2);

        crate::ui::renderer::UiRect {
            x,
            y,
            width,
            height,
        }
    }

    fn sidebar_rect(&self) -> crate::ui::renderer::UiRect {
        let panel = self.settings_panel_rect();
        crate::ui::renderer::UiRect {
            x: panel.x,
            y: panel.y,
            width: SIDEBAR_WIDTH.min(panel.width / 3),
            height: panel.height,
        }
    }

    fn content_rect(&self) -> crate::ui::renderer::UiRect {
        let panel = self.settings_panel_rect();
        let sidebar = self.sidebar_rect();
        crate::ui::renderer::UiRect {
            x: sidebar.x + sidebar.width + CONTENT_PADDING,
            y: panel.y,
            width: panel
                .width
                .saturating_sub(sidebar.width + CONTENT_PADDING * 2),
            height: panel.height,
        }
    }

    fn settings_item_rect(&self, index: usize) -> Option<crate::ui::renderer::UiRect> {
        if index >= AppearanceSettingKey::all().len() {
            return None;
        }
        let content = self.content_rect();
        let y = content.y + 92 + index * (ROW_HEIGHT + ROW_GAP);
        Some(crate::ui::renderer::UiRect {
            x: content.x,
            y,
            width: content.width,
            height: ROW_HEIGHT,
        })
    }

    fn primary_control_rect(
        &self,
        row: crate::ui::renderer::UiRect,
    ) -> crate::ui::renderer::UiRect {
        crate::ui::renderer::UiRect {
            x: row.x + row.width.saturating_sub(CONTROL_WIDTH + 14),
            y: row.y + (row.height.saturating_sub(CONTROL_HEIGHT)) / 2,
            width: CONTROL_WIDTH,
            height: CONTROL_HEIGHT,
        }
    }

    fn secondary_control_rect(
        &self,
        key: AppearanceSettingKey,
        primary: crate::ui::renderer::UiRect,
    ) -> Option<crate::ui::renderer::UiRect> {
        match key {
            AppearanceSettingKey::LineHeight | AppearanceSettingKey::FontSize => {
                Some(crate::ui::renderer::UiRect {
                    x: primary.x + primary.width.saturating_sub(STEPPER_BUTTON_WIDTH),
                    y: primary.y,
                    width: STEPPER_BUTTON_WIDTH,
                    height: primary.height,
                })
            }
            _ => None,
        }
    }

    fn point_in_rect(position: PhysicalPosition<f64>, rect: crate::ui::renderer::UiRect) -> bool {
        position.x >= rect.x as f64
            && position.x < (rect.x + rect.width) as f64
            && position.y >= rect.y as f64
            && position.y < (rect.y + rect.height) as f64
    }

    fn click_target_at(&self, position: PhysicalPosition<f64>) -> Option<SettingsClickTarget> {
        for (idx, key) in AppearanceSettingKey::all().iter().copied().enumerate() {
            let row = self.settings_item_rect(idx)?;
            if !Self::point_in_rect(position, row) {
                continue;
            }

            let primary = self.primary_control_rect(row);
            let secondary = self.secondary_control_rect(key, primary);
            if let Some(secondary) = secondary
                && Self::point_in_rect(position, secondary)
            {
                return Some(SettingsClickTarget::Secondary(key));
            }
            if Self::point_in_rect(position, primary) {
                return Some(SettingsClickTarget::Primary(key));
            }
            return Some(SettingsClickTarget::Row(key));
        }
        None
    }

    pub(super) fn handle_settings_click(&mut self, position: PhysicalPosition<f64>) -> bool {
        let button = self.settings_button_rect();
        if Self::point_in_rect(position, button) {
            self.toggle_settings_panel();
            return true;
        }

        if !self.interaction.settings_panel_open {
            return false;
        }

        if let Some(target) = self.click_target_at(position) {
            self.apply_settings_target(target);
            return true;
        }

        let panel = self.settings_panel_rect();
        if !Self::point_in_rect(position, panel) {
            self.close_settings_panel();
            return true;
        }

        true
    }

    fn apply_settings_target(&mut self, target: SettingsClickTarget) {
        match target {
            SettingsClickTarget::Row(key) | SettingsClickTarget::Primary(key) => match key {
                AppearanceSettingKey::FontFamily => {
                    self.renderer.cycle_font_family();
                    self.refit_terminal_to_renderer();
                }
                AppearanceSettingKey::Ligatures => {
                    self.renderer.toggle_ligatures();
                    self.sync_renderer_from_terminal(true);
                }
                AppearanceSettingKey::LineHeight => {
                    self.renderer.cycle_line_height();
                    self.refit_terminal_to_renderer();
                }
                AppearanceSettingKey::FontSize => {
                    self.renderer.set_font_size(self.renderer.font_size - 2.0);
                    self.refit_terminal_to_renderer();
                }
                AppearanceSettingKey::ResetAppearance => {
                    self.renderer.reset_appearance();
                    self.refit_terminal_to_renderer();
                }
            },
            SettingsClickTarget::Secondary(key) => match key {
                AppearanceSettingKey::LineHeight => {
                    self.renderer.cycle_line_height();
                    self.refit_terminal_to_renderer();
                }
                AppearanceSettingKey::FontSize => {
                    self.renderer.set_font_size(self.renderer.font_size + 2.0);
                    self.refit_terminal_to_renderer();
                }
                _ => {}
            },
        }
    }

    pub(super) fn is_position_in_settings_ui(&self, position: PhysicalPosition<f64>) -> bool {
        let button = self.settings_button_rect();
        if Self::point_in_rect(position, button) {
            return true;
        }
        self.interaction.settings_panel_open
            && Self::point_in_rect(position, self.settings_panel_rect())
    }

    pub(super) fn settings_panel_info(&self) -> SettingsPanelRenderInfo {
        let button_rect = self.settings_button_rect();
        let panel_rect = self.settings_panel_rect();
        let sidebar_rect = self.sidebar_rect();
        let content_rect = self.content_rect();
        let mut items = Vec::with_capacity(AppearanceSettingKey::all().len());

        if self.interaction.settings_panel_open {
            for (idx, key) in AppearanceSettingKey::all().iter().copied().enumerate() {
                if let Some(rect) = self.settings_item_rect(idx) {
                    let primary_rect = self.primary_control_rect(rect);
                    items.push(crate::ui::renderer::SettingsItemRenderInfo {
                        title: key.title().to_string(),
                        detail: key.detail().to_string(),
                        value: key.value(self),
                        control: key.control(self),
                        is_hovered: Self::point_in_rect(self.interaction.mouse_position, rect),
                        rect,
                        primary_rect,
                        secondary_rect: self.secondary_control_rect(key, primary_rect),
                    });
                }
            }
        }

        SettingsPanelRenderInfo {
            is_open: self.interaction.settings_panel_open,
            button_rect,
            button_hovered: Self::point_in_rect(self.interaction.mouse_position, button_rect),
            panel_rect,
            sidebar_rect,
            content_rect,
            items,
        }
    }
}
