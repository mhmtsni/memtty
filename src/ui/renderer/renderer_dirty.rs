use super::{CURSOR_STYLE_BLOCK, Cell, CellKey, CursorCacheKey, Renderer};

pub(super) struct DirtyInfo {
    pub content_dirty: Vec<bool>,
    pub any_dirty: bool,
    pub any_content_dirty: bool,
    pub needs_text_rebuild: bool,
}

/// Computes per-row dirty flags and the downstream rebuild decisions.
pub(super) fn compute_dirty_info(
    renderer: &Renderer,
    rows: &[&Vec<Cell>],
    cursor_changed: bool,
    new_cursor_state: Option<CursorCacheKey>,
    content_changed_hint: bool,
) -> DirtyInfo {
    let row_count = rows.len();

    let mut content_dirty = vec![renderer.full_rebuild; row_count];
    let mut dirty = vec![renderer.full_rebuild; row_count];

    if !renderer.full_rebuild && !content_changed_hint {
        for (row_i, row) in rows.iter().enumerate() {
            let cache = &renderer.last_grid[row_i];

            // Content changed?
            if cache.len() != row.len() {
                content_dirty[row_i] = true;
                dirty[row_i] = true;
                continue;
            }

            for (cell, &key) in row.iter().zip(cache.iter()) {
                if CellKey::from_cell(cell) != key {
                    content_dirty[row_i] = true;
                    dirty[row_i] = true;
                    break;
                }
            }
        }

        // Cursor movement dirties the rows it touches.
        if cursor_changed {
            if let Some(old) = renderer.last_cursor {
                let old_row = old.row;
                if old_row < row_count {
                    dirty[old_row] = true;
                }
            }

            if let Some(new_cursor) = new_cursor_state {
                let new_row = new_cursor.row;
                if new_row < row_count {
                    dirty[new_row] = true;
                }
            }
        }
    } else if !renderer.full_rebuild && content_changed_hint {
        // External hint says "content likely changed", so we pessimistically
        // mark everything dirty and avoid the expensive diff scan.
        content_dirty.fill(true);
        dirty.fill(true);
    }

    let any_dirty = dirty.iter().any(|&d| d);
    if !any_dirty {
        return DirtyInfo {
            content_dirty,
            any_dirty,
            any_content_dirty: false,
            needs_text_rebuild: false,
        };
    }

    let any_content_dirty = content_dirty.iter().any(|&d| d);
    let prev_block_cursor_visible = renderer
        .last_cursor
        .map(|c| c.style == CURSOR_STYLE_BLOCK && c.blink_on)
        .unwrap_or(false);
    let new_block_cursor_visible = new_cursor_state
        .map(|c| c.style == CURSOR_STYLE_BLOCK && c.blink_on)
        .unwrap_or(false);

    // Block cursor inverts the glyph color under the cursor, so if the
    // cursor highlight appears/disappears at the same position, text must
    // still be rebuilt.
    let cursor_affects_text = prev_block_cursor_visible || new_block_cursor_visible;
    let needs_text_rebuild = any_content_dirty || (cursor_changed && cursor_affects_text);

    DirtyInfo {
        content_dirty,
        any_dirty,
        any_content_dirty,
        needs_text_rebuild,
    }
}
