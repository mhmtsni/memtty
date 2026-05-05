use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ShortcutIntent {
    CloseSettings,
    Paste,
    ToggleFullscreen,
    ToggleSettingsPanel,
    FontSizeStep(f32),
    FontSizeReset,
    NewTab,
    CloseTab,
    Copy,
    SelectAll,
    SwitchToTab(usize),
}

pub(super) fn shortcut_intent_for_key(
    event: &KeyEvent,
    modifiers: ModifiersState,
    settings_panel_open: bool,
) -> Option<ShortcutIntent> {
    if event.state != ElementState::Pressed {
        return None;
    }

    if matches!(event.logical_key, Key::Named(NamedKey::Escape)) && settings_panel_open {
        return Some(ShortcutIntent::CloseSettings);
    }

    if modifiers.super_key()
        && matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"))
    {
        return Some(ShortcutIntent::Paste);
    }

    if matches!(event.logical_key, Key::Named(NamedKey::Enter)) && modifiers.super_key() {
        return Some(ShortcutIntent::ToggleFullscreen);
    }

    if !modifiers.super_key() {
        return None;
    }

    let Key::Character(c) = &event.logical_key else {
        return None;
    };

    shortcut_intent_for_super_char(c)
}

fn shortcut_intent_for_super_char(c: &str) -> Option<ShortcutIntent> {
    match c.to_lowercase().as_str() {
        "," => Some(ShortcutIntent::ToggleSettingsPanel),
        "+" | "=" => Some(ShortcutIntent::FontSizeStep(2.0)),
        "-" => Some(ShortcutIntent::FontSizeStep(-2.0)),
        "0" => Some(ShortcutIntent::FontSizeReset),
        "t" => Some(ShortcutIntent::NewTab),
        "w" => Some(ShortcutIntent::CloseTab),
        "c" => Some(ShortcutIntent::Copy),
        "a" => Some(ShortcutIntent::SelectAll),
        _ => {
            if c.len() == 1 {
                if let Some(ch) = c.chars().next() {
                    if ch.is_ascii_digit() && ch != '0' {
                        return Some(ShortcutIntent::SwitchToTab(
                            ch.to_digit(10).unwrap() as usize - 1,
                        ));
                    }
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_super_t_to_new_tab() {
        assert_eq!(
            shortcut_intent_for_super_char("t"),
            Some(ShortcutIntent::NewTab)
        );
    }

    #[test]
    fn maps_super_digit_to_tab_switch() {
        assert_eq!(
            shortcut_intent_for_super_char("3"),
            Some(ShortcutIntent::SwitchToTab(2))
        );
    }
}
