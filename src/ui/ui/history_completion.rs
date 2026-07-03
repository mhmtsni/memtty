use std::collections::HashSet;
use std::path::PathBuf;

use glyphon::Color;

use super::*;
use crate::terminal::Cell;

const MAX_HISTORY_MATCHES: usize = 200;
const HISTORY_CACHE_LIMIT: usize = 1000;
const PREVIEW_FG: Color = Color::rgb(120, 126, 138);

impl MyApp {
    pub(super) fn cycle_history_completion(&mut self) -> bool {
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };

        let input_line = tab.input_line.clone();
        let prefix = tab
            .history_completion
            .as_ref()
            .and_then(|state| {
                state
                    .matches
                    .get(state.index)
                    .filter(|command| *command == &input_line)
                    .map(|_| state.prefix.clone())
            })
            .unwrap_or_else(|| input_line.clone());

        if prefix.trim().is_empty() {
            tab.history_completion = None;
            return false;
        }

        let next_command = match tab.history_completion.as_mut() {
            Some(state) if state.prefix == prefix && !state.matches.is_empty() => {
                state.index = (state.index + 1) % state.matches.len();
                state.matches[state.index].clone()
            }
            _ => {
                let matches = history_matches_for_prefix(&prefix, &tab.shell_history);
                let Some(first) = matches.first().cloned() else {
                    tab.history_completion = None;
                    tab.history_preview = None;
                    return false;
                };

                tab.history_completion = Some(HistoryCompletionState {
                    prefix,
                    matches,
                    index: 0,
                });
                first
            }
        };

        tab.input_line = next_command.clone();
        tab.history_preview = None;

        self.clear_selection();
        self.send_to_pty(PtyInput::Data(replace_shell_line_bytes(&next_command)));
        self.reset_scrollback_view();
        true
    }

    pub(super) fn record_key_input(&mut self, event: &KeyEvent, bytes: &[u8]) {
        let modifiers = self.modifiers;
        let Some(tab) = self.active_tab_mut() else {
            return;
        };

        tab.history_completion = None;

        match &event.logical_key {
            Key::Named(NamedKey::Enter) => tab.input_line.clear(),
            Key::Named(NamedKey::Backspace) if modifiers.super_key() => tab.input_line.clear(),
            Key::Named(NamedKey::Backspace) => {
                tab.input_line.pop();
            }
            Key::Named(NamedKey::Delete)
            | Key::Named(NamedKey::ArrowUp)
            | Key::Named(NamedKey::ArrowDown)
            | Key::Named(NamedKey::ArrowRight)
            | Key::Named(NamedKey::ArrowLeft)
            | Key::Named(NamedKey::Home)
            | Key::Named(NamedKey::End)
            | Key::Named(NamedKey::PageUp)
            | Key::Named(NamedKey::PageDown) => tab.input_line.clear(),
            Key::Character(c) if modifiers.control_key() => match c.to_lowercase().as_str() {
                "u" => tab.input_line.clear(),
                "w" => trim_last_word(&mut tab.input_line),
                _ => {}
            },
            _ if bytes == b"\x15" => tab.input_line.clear(),
            _ => {}
        }

        update_history_preview(tab);
    }

    pub(super) fn record_text_input(&mut self, text: &str) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };

        tab.history_completion = None;
        tab.input_line.push_str(text);
        update_history_preview(tab);
    }

    pub(super) fn current_history_preview(&self) -> Option<&str> {
        self.active_tab()?.history_preview.as_deref()
    }
}

pub(super) fn append_history_commands(history: &mut Vec<String>, commands: Vec<String>) {
    for command in commands {
        if history.last() == Some(&command) {
            continue;
        }
        history.push(command);
    }

    if history.len() > HISTORY_CACHE_LIMIT {
        history.drain(..history.len() - HISTORY_CACHE_LIMIT);
    }
}

fn update_history_preview(tab: &mut Tab) {
    tab.history_preview = first_history_match(&tab.input_line, &tab.shell_history)
        .and_then(|command| command.strip_prefix(&tab.input_line).map(str::to_string))
        .filter(|suffix| !suffix.is_empty());
}

fn first_history_match(prefix: &str, live_history: &[String]) -> Option<String> {
    if prefix.trim().is_empty() {
        return None;
    }

    history_matches_for_prefix(prefix, live_history)
        .into_iter()
        .next()
}

fn history_matches_for_prefix(prefix: &str, live_history: &[String]) -> Vec<String> {
    if prefix.trim().is_empty() {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut matches = Vec::new();
    let file_history = history_commands();

    for command in live_history.iter().rev().chain(file_history.iter().rev()) {
        if command == prefix || !command.starts_with(prefix) || !seen.insert(command.clone()) {
            continue;
        }

        matches.push(command.clone());
        if matches.len() >= MAX_HISTORY_MATCHES {
            break;
        }
    }

    matches
}

fn history_commands() -> Vec<String> {
    let mut commands = Vec::new();

    for path in history_paths() {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };

        commands.extend(contents.lines().filter_map(parse_history_line));
    }

    commands
}

fn history_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(histfile) = std::env::var("HISTFILE")
        && !histfile.is_empty()
    {
        paths.push(PathBuf::from(histfile));
    }

    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        paths.push(home.join(".zsh_history"));
        paths.push(home.join(".bash_history"));
    }

    paths
}

fn parse_history_line(line: &str) -> Option<String> {
    let command = if line.starts_with(": ") {
        line.split_once(';').map(|(_, command)| command)?
    } else {
        line
    };

    let command = command.trim();
    (!command.is_empty()).then(|| command.to_string())
}

fn replace_shell_line_bytes(command: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(command.len() + 1);
    bytes.push(0x15); // Ctrl-U: clear the shell's current editable line.
    bytes.extend_from_slice(command.as_bytes());
    bytes
}

fn trim_last_word(line: &mut String) {
    let trimmed_len = line.trim_end().len();
    line.truncate(trimmed_len);

    while let Some(ch) = line.chars().next_back() {
        if ch.is_whitespace() {
            break;
        }
        line.pop();
    }
}

pub(super) fn draw_history_preview(row: &mut [Cell], cursor_x: usize, preview: &str) {
    if preview.is_empty() || cursor_x >= row.len() {
        return;
    }

    for (cell, ch) in row.iter_mut().skip(cursor_x).zip(preview.chars()) {
        *cell = Cell {
            c: ch,
            text: ch.to_string().into(),
            wide_continuation: false,
            hyperlink: None,
            is_link_hovered: false,
            fg: PREVIEW_FG,
            bg: cell.bg,
            is_selected: false,
            style: 0,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zsh_extended_history_lines() {
        assert_eq!(
            parse_history_line(": 1710000000:0;ssh prod"),
            Some("ssh prod".to_string())
        );
    }

    #[test]
    fn parses_plain_bash_history_lines() {
        assert_eq!(
            parse_history_line("cargo test"),
            Some("cargo test".to_string())
        );
    }

    #[test]
    fn ignores_empty_history_lines() {
        assert_eq!(parse_history_line(""), None);
        assert_eq!(parse_history_line("   "), None);
    }

    #[test]
    fn replacement_clears_line_then_writes_command() {
        assert_eq!(
            replace_shell_line_bytes("ssh prod"),
            b"\x15ssh prod".to_vec()
        );
    }

    #[test]
    fn appends_history_commands_without_adjacent_duplicates() {
        let mut history = vec!["ssh old".to_string()];
        append_history_commands(
            &mut history,
            vec!["ssh old".to_string(), "ssh prod".to_string()],
        );

        assert_eq!(history, vec!["ssh old", "ssh prod"]);
    }
}
