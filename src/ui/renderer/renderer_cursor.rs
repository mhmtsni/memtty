use super::{CursorRenderInfo, CursorRenderStyle};

pub(super) fn render_cursor_overlay(
    renderer: &mut super::Renderer,
    cursor: Option<CursorRenderInfo>,
) {
    let Some(cursor) = cursor else {
        return;
    };

    if !cursor.blink_on {
        return;
    }

    let cursor_alpha = 1.0;
    match cursor.style {
        CursorRenderStyle::Block => {
            renderer.push_rect_cells(cursor.col, cursor.row, 1, 1, cursor.color, cursor_alpha)
        }
        CursorRenderStyle::Underline => {
            let underline_height = (renderer.line_height * 0.12).max(2.0);
            renderer.push_rect_pixels(
                cursor.col as f32 * renderer.cell_width,
                (cursor.row as f32 + 1.0) * renderer.line_height - underline_height,
                renderer.cell_width,
                underline_height,
                cursor.color,
                cursor_alpha,
            );
        }
        CursorRenderStyle::Bar => {
            let bar_width = (renderer.cell_width * 0.12).max(2.0);
            renderer.push_rect_pixels(
                cursor.col as f32 * renderer.cell_width,
                cursor.row as f32 * renderer.line_height,
                bar_width,
                renderer.line_height,
                cursor.color,
                cursor_alpha,
            );
        }
    }
}
