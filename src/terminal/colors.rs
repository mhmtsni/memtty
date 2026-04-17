use glyphon::Color;

pub(super) const MAX_SCROLLBACK: usize = 1500;

// Defaults match the previous monolithic module.
pub(super) const DEFAULT_FG: Color = Color::rgb(229, 229, 229);
pub(super) const DEFAULT_BG: Color = Color::rgb(20, 25, 31);

pub(super) fn default_palette_256() -> [Color; 256] {
    let mut palette = [Color::rgb(0, 0, 0); 256];
    for i in 0..=u8::MAX {
        palette[i as usize] = xterm_color_from_256(i);
    }
    palette
}

fn xterm_color_from_256(n: u8) -> Color {
    match n {
        0 => Color::rgb(0, 0, 0),
        1 => Color::rgb(205, 0, 0),
        2 => Color::rgb(0, 205, 0),
        3 => Color::rgb(205, 205, 0),
        4 => Color::rgb(0, 0, 238),
        5 => Color::rgb(205, 0, 205),
        6 => Color::rgb(0, 205, 205),
        7 => Color::rgb(229, 229, 229),
        8 => Color::rgb(127, 127, 127),
        9 => Color::rgb(255, 85, 85),
        10 => Color::rgb(85, 255, 85),
        11 => Color::rgb(255, 255, 85),
        12 => Color::rgb(85, 85, 255),
        13 => Color::rgb(255, 85, 255),
        14 => Color::rgb(85, 255, 255),
        15 => Color::rgb(255, 255, 255),
        16..=231 => {
            let n = n - 16;
            let b = n % 6;
            let g = (n / 6) % 6;
            let r = n / 36;
            let to_val = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color::rgb(to_val(r), to_val(g), to_val(b))
        }
        232..=255 => {
            let v = 8 + (n - 232) * 10;
            Color::rgb(v, v, v)
        }
    }
}

pub(super) fn clamp_u16_to_u8(v: u16) -> u8 {
    v.min(u8::MAX as u16) as u8
}

pub(super) fn parse_sgr_extended_color_group(
    group: &[u16],
    palette_256: &[Color; 256],
) -> Option<Color> {
    match group.get(1).copied() {
        // 38:5:n or 48:5:n
        Some(5) => group
            .get(2)
            .copied()
            .map(clamp_u16_to_u8)
            .map(|idx| palette_256[idx as usize]),
        // 38:2:R:G:B, 38:2::R:G:B, or 38:2:color_space:R:G:B
        Some(2) => {
            if group.len() >= 6 {
                Some(Color::rgb(
                    clamp_u16_to_u8(group[3]),
                    clamp_u16_to_u8(group[4]),
                    clamp_u16_to_u8(group[5]),
                ))
            } else if group.len() >= 5 {
                Some(Color::rgb(
                    clamp_u16_to_u8(group[2]),
                    clamp_u16_to_u8(group[3]),
                    clamp_u16_to_u8(group[4]),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

// Parse an X11 / xterm color specification such as `#rrggbb` or `rgb:rr/gg/bb`.
pub(super) fn parse_color_spec(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        // #rgb or #rrggbb
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            _ => None,
        }
    } else if let Some(rest) = s.strip_prefix("rgb:") {
        // rgb:rr/gg/bb  (each component 1–4 hex digits; we take the top 2)
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() != 3 {
            return None;
        }
        let component = |p: &str| -> Option<u8> {
            let clamped = &p[..p.len().min(2)];
            u8::from_str_radix(clamped, 16).ok()
        };
        Some(Color::rgb(
            component(parts[0])?,
            component(parts[1])?,
            component(parts[2])?,
        ))
    } else {
        None
    }
}
