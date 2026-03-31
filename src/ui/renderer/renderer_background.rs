use super::{effective_colors, Cell};

pub(super) fn rebuild_background_geometry(
    renderer: &mut super::Renderer,
    rows: &[&Vec<Cell>],
) {
    renderer.solid_vertices.clear();

    for (row_i, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }

        let mut run_start = 0usize;
        let mut run_bg = effective_colors(&row[0]).1;

        for col in 1..=row.len() {
            let next_bg = if col < row.len() {
                effective_colors(&row[col]).1
            } else {
                super::Color::rgb(0, 0, 0)
            };

            if col == row.len() || next_bg.0 != run_bg.0 {
                // Background pass already clears to black, so skip pure-black runs.
                if run_bg.0 != super::Color::rgb(0, 0, 0).0 {
                    renderer.push_rect_cells(run_start, row_i, col - run_start, 1, run_bg, 1.0);
                }
                run_start = col;
                run_bg = next_bg;
            }
        }
    }

    renderer.background_vertex_count = renderer.solid_vertices.len();
}

