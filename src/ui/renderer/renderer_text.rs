use super::{Attrs, Cell, Color};
use super::{attrs_equal, build_attrs, contrast_text_color, effective_colors, font_family};

pub(super) fn rebuild_text_spans(
    renderer: &mut super::Renderer,
    rows: &[&Vec<Cell>],
    cursor_block_cell: Option<(usize, usize, Color)>,
) {
    // Clear existing cached rich-text spans; we rebuild the active ones.
    for (s, _) in renderer.spans_cache.iter_mut() {
        s.clear();
    }

    let row_count = rows.len();
    let mut span_count = 0usize;

    for (row_i, row) in rows.iter().enumerate() {
        let last_non_space = row
            .iter()
            .rposition(|c| !c.is_blank())
            .map(|i| i + 1)
            .unwrap_or(0);

        for (col_i, cell) in row[..last_non_space].iter().enumerate() {
            let (mut fg, _bg) = effective_colors(cell);

            if let Some((cursor_col, cursor_row, cursor_color)) = cursor_block_cell
                && cursor_row == row_i
                && cursor_col == col_i
            {
                fg = contrast_text_color(cursor_color);
            }

            let attrs = build_attrs(cell, fg, renderer.font_family_name);
            let text = cell.display_text();
            if text.is_empty() {
                continue;
            }

            if span_count > 0 && attrs_equal(&renderer.spans_cache[span_count - 1].1, &attrs) {
                renderer.spans_cache[span_count - 1].0.push_str(text);
            } else {
                if span_count < renderer.spans_cache.len() {
                    renderer.spans_cache[span_count].0.clear();
                    renderer.spans_cache[span_count].0.push_str(text);
                    renderer.spans_cache[span_count].1 = attrs;
                } else {
                    renderer.spans_cache.push((text.to_string(), attrs));
                }
                span_count += 1;
            }
        }

        // Newline between rows.
        if row_i + 1 < row_count {
            if span_count > 0 {
                renderer.spans_cache[span_count - 1].0.push('\n');
            } else {
                // Empty row: create a one-character newline span.
                let newline_attrs = Attrs::new()
                    .family(font_family(renderer.font_family_name))
                    .color(Color::rgb(255, 255, 255));

                if span_count < renderer.spans_cache.len() {
                    renderer.spans_cache[span_count].0.clear();
                    renderer.spans_cache[span_count].0.push('\n');
                    renderer.spans_cache[span_count].1 = newline_attrs;
                } else {
                    renderer.spans_cache.push(("\n".to_string(), newline_attrs));
                }
                span_count += 1;
            }
        }
    }

    let active_span_count = span_count.min(renderer.spans_cache.len());
    let active_spans = &renderer.spans_cache[..active_span_count];
    let shaping_mode = renderer.shaping_mode();
    renderer.buffer.set_rich_text(
        &mut renderer.font_system,
        active_spans.iter().map(|(s, a)| (s.as_str(), a.clone())),
        &Attrs::new()
            .family(font_family(renderer.font_family_name))
            .color(Color::rgb(229, 229, 229)),
        shaping_mode,
        None::<glyphon::cosmic_text::Align>,
    );
    renderer.needs_shape = true;
}
