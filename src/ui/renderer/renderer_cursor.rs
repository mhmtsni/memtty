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
                cursor.col as f32 * renderer.cell_width + renderer.content_left(),
                (cursor.row as f32 + 1.0) * renderer.line_height - underline_height
                    + renderer.content_top(),
                renderer.cell_width,
                underline_height,
                cursor.color,
                cursor_alpha,
            );
        }
        CursorRenderStyle::Bar => {
            let bar_width = (renderer.cell_width * 0.12).max(2.0);
            renderer.push_rect_pixels(
                cursor.col as f32 * renderer.cell_width + renderer.content_left(),
                cursor.row as f32 * renderer.line_height + renderer.content_top(),
                bar_width,
                renderer.line_height,
                cursor.color,
                cursor_alpha,
            );
        }
        CursorRenderStyle::Unfocused => {
            // Draw a thin outline (frame) instead of a filled block.
            // Using pixel-space avoids the 1x1-cell case looking like a normal block.
            let x = cursor.col as f32 * renderer.cell_width + renderer.content_left();
            let y = cursor.row as f32 * renderer.line_height + renderer.content_top();
            let w = renderer.cell_width;
            let h = renderer.line_height;

            let alpha = cursor_alpha * 0.6;
            let thickness = (w.min(h) * 0.12).max(1.5);

            // Top
            renderer.push_rect_pixels(x, y, w, thickness, cursor.color, alpha);
            // Bottom
            renderer.push_rect_pixels(x, y + h - thickness, w, thickness, cursor.color, alpha);
            // Left
            renderer.push_rect_pixels(x, y, thickness, h, cursor.color, alpha);
            // Right
            renderer.push_rect_pixels(x + w - thickness, y, thickness, h, cursor.color, alpha);
        }
    }
}
