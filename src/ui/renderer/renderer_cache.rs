use super::{Cell, CellKey};

pub(super) fn update_last_grid_snapshot(
    renderer: &mut super::Renderer,
    rows: &[&Vec<Cell>],
    content_dirty: &[bool],
) {
    for (row_i, row) in rows.iter().enumerate() {
        if !content_dirty[row_i] {
            continue;
        }

        let cache_row = &mut renderer.last_grid[row_i];
        cache_row.resize(
            row.len(),
            CellKey {
                c: ' ',
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
