use super::{Performer, parse_color_spec};

impl Performer {
    pub(super) fn dispatch_osc(&mut self, params: &[&[u8]]) {
        let cmd = params
            .first()
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(u32::MAX);

        let arg = |i: usize| -> &[u8] { params.get(i).copied().unwrap_or(b"") };

        match cmd {
            0..=2 => self.set_window_title(params),
            4 => self.set_palette_entries(params),
            8 => {
                let uri = arg(2);
                if uri.is_empty() {
                    self.current_hyperlink = None;
                } else {
                    self.current_hyperlink = Some(String::from_utf8_lossy(uri).to_string().into());
                }
            }
            10 => self.set_default_foreground(arg(1)),
            11 => self.set_default_background(arg(1)),
            52 => {
                if let Ok(spec) = std::str::from_utf8(arg(2))
                    && spec == "?"
                {
                    let target = std::str::from_utf8(arg(1)).unwrap_or("c");
                    self.queue_pty_reply(format!("\x1b]52;{};\x07", target).into_bytes());
                }
            }
            133 => {}
            777 => self.handle_terminal_private_osc(params),
            _ => {}
        }
    }

    fn handle_terminal_private_osc(&mut self, params: &[&[u8]]) {
        if params.get(1).copied() != Some(b"history") || params.len() <= 2 {
            return;
        }

        let mut command_bytes = Vec::new();
        for (i, part) in params.iter().enumerate().skip(2) {
            if i > 2 {
                command_bytes.push(b';');
            }
            command_bytes.extend_from_slice(part);
        }

        self.queue_history_command(String::from_utf8_lossy(&command_bytes).to_string());
    }

    fn set_window_title(&mut self, params: &[&[u8]]) {
        if params.len() <= 1 {
            self.title.clear();
            return;
        }

        let mut title_bytes = Vec::new();
        for (i, part) in params.iter().enumerate().skip(1) {
            if i > 1 {
                title_bytes.push(b';');
            }
            title_bytes.extend_from_slice(part);
        }
        self.title = String::from_utf8_lossy(&title_bytes).to_string();
    }

    fn set_palette_entries(&mut self, params: &[&[u8]]) {
        let arg = |i: usize| -> &[u8] { params.get(i).copied().unwrap_or(b"") };
        let mut i = 1;
        while i + 1 < params.len() {
            let idx_bytes = arg(i);
            let spec = arg(i + 1);
            if let (Ok(idx_str), Ok(spec_str)) =
                (std::str::from_utf8(idx_bytes), std::str::from_utf8(spec))
                && let Ok(n) = idx_str.parse::<u8>()
                && let Some(color) = parse_color_spec(spec_str)
            {
                self.palette_256[n as usize] = color;
            }
            i += 2;
        }
    }

    fn set_default_foreground(&mut self, spec_bytes: &[u8]) {
        if let Ok(spec) = std::str::from_utf8(spec_bytes)
            && spec != "?"
            && let Some(color) = parse_color_spec(spec)
        {
            let old = self.default_fg;
            self.default_fg = color;
            if self.current_fg == old {
                self.current_fg = color;
            }
        }
    }

    fn set_default_background(&mut self, spec_bytes: &[u8]) {
        if let Ok(spec) = std::str::from_utf8(spec_bytes)
            && spec != "?"
            && let Some(color) = parse_color_spec(spec)
        {
            let old = self.default_bg;
            self.default_bg = color;
            if self.current_bg == old {
                self.current_bg = color;
            }
        }
    }
}
