#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Charset {
    Ascii,
    DecSpecialGraphics,
}

pub(super) fn charset_from_designator(designator: u8) -> Charset {
    match designator {
        b'0' => Charset::DecSpecialGraphics,
        _ => Charset::Ascii,
    }
}

pub(super) fn map_dec_special_graphics(c: char) -> char {
    match c {
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'q' => '─',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}

