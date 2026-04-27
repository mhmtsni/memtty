use super::*;

impl MyApp {
    pub fn handle_key_event(&mut self, event: KeyEvent, proxy: Option<EventLoopProxy<Message>>) {
        if event.state != ElementState::Pressed {
            return;
        }

        if self.modifiers.super_key()
            && matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"))
        {
            self.handle_paste();
            return;
        }

        if matches!(event.logical_key, Key::Named(NamedKey::Enter)) && self.modifiers.super_key() {
            self.full_screen = !self.full_screen;
            let mode = if self.full_screen {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            };
            self.window.set_fullscreen(mode);
            return;
        }

        if self.modifiers.super_key() {
            if let Key::Character(c) = &event.logical_key {
                match c.to_lowercase().as_str() {
                    "+" | "=" => {
                        let new_size = self.renderer.font_size + 2.0;
                        self.renderer.set_font_size(new_size);
                        self.refit_terminal_to_renderer();
                        return;
                    }
                    "-" => {
                        let new_size = self.renderer.font_size - 2.0;
                        self.renderer.set_font_size(new_size);
                        self.refit_terminal_to_renderer();
                        return;
                    }
                    "0" => {
                        self.renderer.reset_font_size();
                        self.refit_terminal_to_renderer();
                        return;
                    }
                    "t" => {
                        if let Some(proxy) = proxy.clone() {
                            self.create_new_tab(proxy);
                        }
                        return;
                    }
                    "w" => {
                        if !self.close_active_tab() {
                            if let Some(proxy) = proxy {
                                let _ = proxy.send_event(Message::Exit);
                            }
                        }
                        return;
                    }
                    "c" => {
                        if self.copy_selection_to_clipboard() {
                            return;
                        }
                        return;
                    }
                    "a" => {
                        if self.select_all() {
                            self.sync_renderer_from_terminal(true);
                        }
                        return;
                    }
                    _ => {
                        if c.len() == 1 {
                            if let Some(ch) = c.chars().next() {
                                if ch.is_ascii_digit() && ch != '0' {
                                    let index = ch.to_digit(10).unwrap() as usize - 1;

                                    if index < self.tabs.len() {
                                        self.active_tab = index;
                                        self.clear_selection();
                                        self.reset_scrollback_view();
                                        self.sync_renderer_from_terminal(true);
                                    }
                                    return;
                                }
                            }
                        }

                        return;
                    }
                }
            }
        }

        if let Some(bytes) = self.map_key_to_bytes(&event) {
            self.clear_selection();
            self.send_to_pty(PtyInput::Data(bytes));
            self.reset_scrollback_view();
            return;
        }

        if let Some(text) = event.text.as_ref() {
            if !text.is_empty() {
                self.clear_selection();
                self.send_to_pty(PtyInput::Data(text.as_bytes().to_vec()));
                self.reset_scrollback_view();
            }
        }
    }

    fn map_key_to_bytes(&mut self, event: &KeyEvent) -> Option<Vec<u8>> {
        let key = &event.logical_key;

        match key {
            Key::Named(NamedKey::ArrowUp) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5A".to_vec())
            }
            Key::Named(NamedKey::ArrowDown) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5B".to_vec())
            }
            Key::Named(NamedKey::ArrowRight) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5C".to_vec())
            }
            Key::Named(NamedKey::ArrowLeft) if self.modifiers.alt_key() => {
                Some(b"\x1b[1;5D".to_vec())
            }
            Key::Named(NamedKey::Backspace) if self.modifiers.alt_key() => {
                Some(b"\x1b\x7f".to_vec())
            }
            Key::Character(c) if self.modifiers.control_key() => match c.to_lowercase().as_str() {
                "a" => Some(b"\x01".to_vec()),
                "b" => Some(b"\x02".to_vec()),
                "c" => Some(b"\x03".to_vec()),
                "d" => Some(b"\x04".to_vec()),
                "e" => Some(b"\x05".to_vec()),
                "f" => Some(b"\x06".to_vec()),
                "g" => Some(b"\x07".to_vec()),
                "h" => Some(b"\x08".to_vec()),
                "i" => Some(b"\x09".to_vec()),
                "j" => Some(b"\x0a".to_vec()),
                "k" => Some(b"\x0b".to_vec()),
                "l" => Some(b"\x0c".to_vec()),
                "m" => Some(b"\x0d".to_vec()),
                "n" => Some(b"\x0e".to_vec()),
                "o" => Some(b"\x0f".to_vec()),
                "p" => Some(b"\x10".to_vec()),
                "q" => Some(b"\x11".to_vec()),
                "r" => Some(b"\x12".to_vec()),
                "s" => Some(b"\x13".to_vec()),
                "t" => Some(b"\x14".to_vec()),
                "u" => Some(b"\x15".to_vec()),
                "v" => Some(b"\x16".to_vec()),
                "w" => Some(b"\x17".to_vec()),
                "x" => Some(b"\x18".to_vec()),
                "y" => Some(b"\x19".to_vec()),
                "z" => Some(b"\x1a".to_vec()),
                "[" => Some(b"\x1b".to_vec()),
                "\\" => Some(b"\x1c".to_vec()),
                "]" => Some(b"\x1d".to_vec()),
                _ => None,
            },

            Key::Named(NamedKey::Backspace) if self.modifiers.super_key() => Some(b"\x15".to_vec()),
            Key::Named(NamedKey::Tab) if self.modifiers.shift_key() => Some(b"\x1b[Z".to_vec()),
            Key::Named(NamedKey::Enter) => Some(b"\r".to_vec()),
            Key::Named(NamedKey::Backspace) => Some(b"\x7f".to_vec()),
            Key::Named(NamedKey::Escape) => Some(b"\x1b".to_vec()),
            Key::Named(NamedKey::Tab) => Some(b"\t".to_vec()),
            Key::Named(NamedKey::Space) => Some(b" ".to_vec()),
            Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
            Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A".to_vec()),
            Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B".to_vec()),
            Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C".to_vec()),
            Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D".to_vec()),
            Key::Named(NamedKey::Home) => Some(b"\x1b[H".to_vec()),
            Key::Named(NamedKey::End) => Some(b"\x1b[F".to_vec()),
            Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
            Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
            Key::Named(NamedKey::F1) => Some(b"\x1bOP".to_vec()),
            Key::Named(NamedKey::F2) => Some(b"\x1bOQ".to_vec()),
            Key::Named(NamedKey::F3) => Some(b"\x1bOR".to_vec()),
            Key::Named(NamedKey::F4) => Some(b"\x1bOS".to_vec()),
            Key::Named(NamedKey::F5) => Some(b"\x1b[15~".to_vec()),
            Key::Named(NamedKey::F6) => Some(b"\x1b[17~".to_vec()),
            Key::Named(NamedKey::F7) => Some(b"\x1b[18~".to_vec()),
            Key::Named(NamedKey::F8) => Some(b"\x1b[19~".to_vec()),
            Key::Named(NamedKey::F9) => Some(b"\x1b[20~".to_vec()),
            Key::Named(NamedKey::F10) => Some(b"\x1b[21~".to_vec()),
            Key::Named(NamedKey::F11) => Some(b"\x1b[23~".to_vec()),
            Key::Named(NamedKey::F12) => Some(b"\x1b[24~".to_vec()),
            _ => None,
        }
    }
}
