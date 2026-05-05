use super::{Cell, CellKey};

pub(super) fn update_last_grid_snapshot(
    renderer: &mut super::Renderer,
    rows: &[&Vec<Cell>],
    content_dirty: &[bool],
) {
    for (row_i, row) in rows.iter().enumerate() {
        if !content_dirty.get(row_i).copied().unwrap_or(true) {
            continue;
        }

        let Some(cache_row) = renderer.last_grid.get_mut(row_i) else {
            continue;
        };
        cache_row.resize(
            row.len(),
            CellKey {
                c: ' ',
                wide_continuation: false,
                text_hash: 0,
                hyperlink_hash: 0,
                is_link_hovered: false,
                fg: 0,
                bg: 0,
                style: 0,
            },
        );

        for (cell, key) in row.iter().zip(cache_row.iter_mut()) {
            *key = CellKey::from_cell(cell);
        }
    }
}
