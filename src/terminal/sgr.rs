use glyphon::Color;

use super::{
    colors::{clamp_u16_to_u8, parse_sgr_extended_color_group},
    performer::Performer,
    style,
};

impl Performer {
    pub(super) fn apply_sgr(&mut self, params: &vte::Params) {
        let grouped_params: Vec<&[u16]> = params.iter().collect();
        if grouped_params.is_empty() {
            self.current_fg = self.default_fg;
            self.current_bg = self.default_bg;
            self.current_style = 0;
            return;
        }

        let mut i = 0;
        while i < grouped_params.len() {
            let group = grouped_params[i];
            let code = group.first().copied().unwrap_or(0);

            match code {
                0 => {
                    self.current_fg = self.default_fg;
                    self.current_bg = self.default_bg;
                    self.current_style = 0;
                }
                1 => self.current_style |= style::BOLD,
                2 => self.current_style |= style::DIM,
                3 => self.current_style |= style::ITALIC,
                4 => self.current_style |= style::UNDERLINE,
                5 | 6 => self.current_style |= style::BLINK,
                7 => self.current_style |= style::REVERSE,
                8 => self.current_style |= style::HIDDEN,
                9 => self.current_style |= style::STRIKETHROUGH,
                21 | 22 => self.current_style &= !(style::BOLD | style::DIM),
                23 => self.current_style &= !style::ITALIC,
                24 => self.current_style &= !style::UNDERLINE,
                25 => self.current_style &= !style::BLINK,
                27 => self.current_style &= !style::REVERSE,
                28 => self.current_style &= !style::HIDDEN,
                29 => self.current_style &= !style::STRIKETHROUGH,

                // Standard foreground (30–37, 39)
                30 => self.current_fg = self.palette_256[0],
                31 => self.current_fg = self.palette_256[1],
                32 => self.current_fg = self.palette_256[2],
                33 => self.current_fg = self.palette_256[3],
                34 => self.current_fg = self.palette_256[4],
                35 => self.current_fg = self.palette_256[5],
                36 => self.current_fg = self.palette_256[6],
                37 => self.current_fg = self.palette_256[7],
                39 => self.current_fg = self.default_fg,

                // Extended foreground: 38;5;n  or  38;2;r;g;b
                38 => {
                    // Colon-form SGR can arrive as one grouped parameter,
                    // e.g. 38:2::R:G:B.
                    if group.len() > 1 {
                        if let Some(color) =
                            parse_sgr_extended_color_group(group, &self.palette_256)
                        {
                            self.current_fg = color;
                        }
                    } else {
                        match grouped_params
                            .get(i + 1)
                            .and_then(|g| g.first())
                            .copied()
                            .unwrap_or(0)
                        {
                            5 if i + 2 < grouped_params.len() => {
                                let n = grouped_params[i + 2].first().copied().unwrap_or(0);
                                self.current_fg = self.palette_256[clamp_u16_to_u8(n) as usize];
                                i += 2;
                            }
                            2 if i + 4 < grouped_params.len() => {
                                let r = grouped_params[i + 2].first().copied().unwrap_or(0);
                                let g = grouped_params[i + 3].first().copied().unwrap_or(0);
                                let b = grouped_params[i + 4].first().copied().unwrap_or(0);
                                self.current_fg = Color::rgb(
                                    clamp_u16_to_u8(r),
                                    clamp_u16_to_u8(g),
                                    clamp_u16_to_u8(b),
                                );
                                i += 4;
                            }
                            _ => {}
                        }
                    }
                }

                // Standard background (40–47, 49)
                40 => self.current_bg = self.palette_256[0],
                41 => self.current_bg = self.palette_256[1],
                42 => self.current_bg = self.palette_256[2],
                43 => self.current_bg = self.palette_256[3],
                44 => self.current_bg = self.palette_256[4],
                45 => self.current_bg = self.palette_256[5],
                46 => self.current_bg = self.palette_256[6],
                47 => self.current_bg = self.palette_256[7],
                49 => self.current_bg = self.default_bg,

                // Extended background: 48;5;n  or  48;2;r;g;b
                48 => {
                    if group.len() > 1 {
                        if let Some(color) =
                            parse_sgr_extended_color_group(group, &self.palette_256)
                        {
                            self.current_bg = color;
                        }
                    } else {
                        match grouped_params
                            .get(i + 1)
                            .and_then(|g| g.first())
                            .copied()
                            .unwrap_or(0)
                        {
                            5 if i + 2 < grouped_params.len() => {
                                let n = grouped_params[i + 2].first().copied().unwrap_or(0);
                                self.current_bg = self.palette_256[clamp_u16_to_u8(n) as usize];
                                i += 2;
                            }
                            2 if i + 4 < grouped_params.len() => {
                                let r = grouped_params[i + 2].first().copied().unwrap_or(0);
                                let g = grouped_params[i + 3].first().copied().unwrap_or(0);
                                let b = grouped_params[i + 4].first().copied().unwrap_or(0);
                                self.current_bg = Color::rgb(
                                    clamp_u16_to_u8(r),
                                    clamp_u16_to_u8(g),
                                    clamp_u16_to_u8(b),
                                );
                                i += 4;
                            }
                            _ => {}
                        }
                    }
                }

                // Bright foreground (90–97)
                90 => self.current_fg = self.palette_256[8],
                91 => self.current_fg = self.palette_256[9],
                92 => self.current_fg = self.palette_256[10],
                93 => self.current_fg = self.palette_256[11],
                94 => self.current_fg = self.palette_256[12],
                95 => self.current_fg = self.palette_256[13],
                96 => self.current_fg = self.palette_256[14],
                97 => self.current_fg = self.palette_256[15],

                // Bright background (100–107)
                100 => self.current_bg = self.palette_256[8],
                101 => self.current_bg = self.palette_256[9],
                102 => self.current_bg = self.palette_256[10],
                103 => self.current_bg = self.palette_256[11],
                104 => self.current_bg = self.palette_256[12],
                105 => self.current_bg = self.palette_256[13],
                106 => self.current_bg = self.palette_256[14],
                107 => self.current_bg = self.palette_256[15],

                _ => {}
            }
            i += 1;
        }
    }
}
