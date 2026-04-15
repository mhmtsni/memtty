use super::*;
use glyphon::Color;

fn term() -> Terminal {
    Terminal::new()
}

#[test]
fn test_bold_flag() {
    let mut t = term();
    t.process(b"\x1b[1mA");
    assert!(t.performer.grid[0][0].style & style::BOLD != 0);
}

#[test]
fn test_reset_bold() {
    let mut t = term();
    t.process(b"\x1b[1mA\x1b[22mB");
    assert!(t.performer.grid[0][0].style & style::BOLD != 0);
    assert!(t.performer.grid[0][1].style & style::BOLD == 0);
}

#[test]
fn test_color() {
    let mut t = term();
    t.process(b"\x1b[31mR\x1b[32mG");
    assert_eq!(t.performer.grid[0][0].fg, Color::rgb(205, 0, 0));
    assert_eq!(t.performer.grid[0][1].fg, Color::rgb(0, 205, 0));
}

#[test]
fn test_cursor_movement() {
    let mut t = term();
    t.process(b"\x1b[5;10H"); // row 5, col 10 (1-based)
    assert_eq!(t.performer.cursor_y, 4);
    assert_eq!(t.performer.cursor_x, 9);
}

#[test]
fn test_auto_wrap() {
    let mut t = term();
    // fill exactly 80 chars then one more
    let line: Vec<u8> = b"A".repeat(80);
    t.process(&line);
    assert!(t.performer.pending_wrap);
    t.process(b"B");
    assert_eq!(t.performer.cursor_y, 1);
    assert_eq!(t.performer.cursor_x, 1);
    assert_eq!(t.performer.grid[1][0].c, 'B');
}

#[test]
fn test_scroll_up_on_newline() {
    let mut t = term();
    // Move to last row and emit a newline
    t.process(b"\x1b[24;1H\n");
    assert_eq!(t.performer.cursor_y, 24 - 1);
    assert_eq!(t.performer.scrollback.len(), 1);
}

#[test]
fn test_save_restore_cursor() {
    let mut t = term();
    t.process(b"\x1b[3;5H"); // move to 3,5
    t.process(b"\x1b7"); // DECSC save
    t.process(b"\x1b[1;1H"); // move elsewhere
    t.process(b"\x1b8"); // DECRC restore
    assert_eq!(t.performer.cursor_y, 2);
    assert_eq!(t.performer.cursor_x, 4);
}

#[test]
fn test_alt_screen() {
    let mut t = term();
    t.process(b"hello");
    t.process(b"\x1b[?1049h"); // enter alt
    assert!(t.performer.in_alt_screen);
    assert_eq!(t.performer.grid[0][0].c, ' '); // blank alt screen
    t.process(b"\x1b[?1049l"); // exit alt
    assert!(!t.performer.in_alt_screen);
    assert_eq!(t.performer.grid[0][0].c, 'h'); // original content back
}

#[test]
fn test_cursor_visibility() {
    let mut t = term();
    t.process(b"\x1b[?25l");
    assert!(!t.performer.cursor_visible);
    t.process(b"\x1b[?25h");
    assert!(t.performer.cursor_visible);
}

#[test]
fn test_scroll_region() {
    let mut t = term();
    t.process(b"\x1b[5;10r"); // set scroll region rows 5–10
    assert_eq!(t.performer.scroll_top, 4);
    assert_eq!(t.performer.scroll_bottom, 9);
    // Cursor should home
    assert_eq!(t.performer.cursor_y, 0);
    assert_eq!(t.performer.cursor_x, 0);
}

#[test]
fn test_partial_top_anchored_scroll_region_does_not_shift_rows_below_region() {
    let mut t = term();

    // Put a marker below the scroll region (row 11, 1-based).
    t.process(b"\x1b[11;1HZ");

    // Scroll only rows 1..10.
    t.process(b"\x1b[1;10r");
    t.process(b"\x1b[10;1H\n");

    // Row 11 should remain untouched by scrolling inside rows 1..10.
    assert_eq!(t.performer.grid[10][0].c, 'Z');
}

#[test]
fn test_erase_line() {
    let mut t = term();
    t.process(b"Hello\x1b[2K"); // write then erase whole line
    for x in 0..80 {
        assert_eq!(t.performer.grid[0][x].c, ' ');
    }
}

#[test]
fn test_erase_display_full_screen() {
    let mut t = term();
    t.process(b"Hello");
    t.process(b"\x1b[2J");
    for row in 0..24 {
        for col in 0..80 {
            assert_eq!(t.performer.grid[row][col].c, ' ');
        }
    }
    assert_eq!(t.performer.cursor_x, 0);
    assert_eq!(t.performer.cursor_y, 0);
}

#[test]
fn test_insert_line_once() {
    let mut t = term();
    t.process(b"\x1b[1;1HA");
    t.process(b"\x1b[2;1HB");
    t.process(b"\x1b[3;1HC");

    t.process(b"\x1b[2;1H\x1b[1L");

    assert_eq!(t.performer.grid[0][0].c, 'A');
    assert_eq!(t.performer.grid[1][0].c, ' ');
    assert_eq!(t.performer.grid[2][0].c, 'B');
    assert_eq!(t.performer.grid[3][0].c, 'C');
}

#[test]
fn test_256_color() {
    let mut t = term();
    t.process(b"\x1b[38;5;196m"); // bright red index 196
    // 196 = 16 + 36*5 + 6*0 + 0  → r=5,g=0,b=0 → rgb(255,0,0)
    assert_eq!(t.performer.current_fg, Color::rgb(255, 0, 0));
}

#[test]
fn test_truecolor() {
    let mut t = term();
    t.process(b"\x1b[38;2;10;20;30m");
    assert_eq!(t.performer.current_fg, Color::rgb(10, 20, 30));
}

#[test]
fn test_truecolor_colon_form() {
    let mut t = term();
    t.process(b"\x1b[38:2::10:20:30m");
    assert_eq!(t.performer.current_fg, Color::rgb(10, 20, 30));
}

#[test]
fn test_reverse_index() {
    let mut t = term();
    t.process(b"\x1b[3;1H"); // row 3
    t.process(b"\x1bM"); // RI: should move up without scroll
    assert_eq!(t.performer.cursor_y, 1);
}

#[test]
fn test_ris_reset() {
    let mut t = term();
    t.process(b"\x1b[1mA\x1bc"); // bold A then RIS
    assert_eq!(t.performer.current_style, 0);
    assert_eq!(t.performer.cursor_x, 0);
    assert_eq!(t.performer.cursor_y, 0);
}

#[test]
fn test_dec_special_graphics_shift() {
    let mut t = term();
    // Designate G1 as DEC special graphics, switch to G1 (SO), then back to G0 (SI).
    t.process(b"\x1b)0\x0eqx\x0fqq");
    assert_eq!(t.performer.grid[0][0].c, '─');
    assert_eq!(t.performer.grid[0][1].c, '│');
    assert_eq!(t.performer.grid[0][2].c, 'q');
    assert_eq!(t.performer.grid[0][3].c, 'q');
}

#[test]
fn test_visible_rows_with_no_scrollback_returns_bottom_of_grid() {
    let t = term();
    let rows = t.visible_rows(0, 2);
    assert_eq!(rows.len(), 2);
    // With a fresh terminal, grid is blank; check we got the last two grid rows.
    assert_eq!(rows[0].len(), 80);
    assert_eq!(rows[1].len(), 80);
    // Pointer identity isn't stable here; check content is blank space.
    assert_eq!(rows[0][0].c, ' ');
    assert_eq!(rows[1][0].c, ' ');
}

#[test]
fn test_visible_rows_with_scrollback_offset_shows_scrollback() {
    let mut t = term();
    // Force at least one scrollback line.
    t.process(b"\x1b[24;1H\n");
    assert_eq!(t.performer.scrollback.len(), 1);

    // When scrolled up, visible window should include scrollback row.
    let rows = t.visible_rows(1, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 80);
}

#[test]
fn test_osc_title_rejoins_semicolons() {
    let mut t = term();
    // OSC 2;hello;world ST
    t.process(b"\x1b]2;hello;world\x07");
    assert_eq!(t.performer.title, "hello;world");
}

#[test]
fn test_parse_color_spec_hash_short_and_long() {
    use super::colors::parse_color_spec;
    assert_eq!(parse_color_spec("#abc"), Some(Color::rgb(0xaa, 0xbb, 0xcc)));
    assert_eq!(
        parse_color_spec("#0a0b0c"),
        Some(Color::rgb(0x0a, 0x0b, 0x0c))
    );
    assert_eq!(parse_color_spec("#zzzzzz"), None);
}

#[test]
fn test_parse_color_spec_rgb_colon_form() {
    use super::colors::parse_color_spec;
    assert_eq!(
        parse_color_spec("rgb:ff/00/80"),
        Some(Color::rgb(0xff, 0x00, 0x80))
    );
    // Accept 1-digit components by taking top 2 chars (here: "a" -> "a").
    assert_eq!(
        parse_color_spec("rgb:a/b/c"),
        Some(Color::rgb(0x0a, 0x0b, 0x0c))
    );
    assert_eq!(parse_color_spec("rgb:ff/00"), None);
}

