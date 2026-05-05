use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionRange {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl SelectionRange {
    pub fn normalized(self) -> Self {
        if self.start <= self.end {
            self
        } else {
            Self {
                start: self.end,
                end: self.start,
            }
        }
    }
}

impl MyApp {
    pub(super) fn clear_selection(&mut self) {
        self.interaction.selection_start = None;
        self.interaction.selection_end = None;
        self.interaction.selection_anchor = None;
    }

    pub(super) fn current_selection_range(&self, total_rows: usize) -> Option<SelectionRange> {
        let (Some(start), Some(end)) = (
            self.interaction.selection_start,
            self.interaction.selection_end,
        ) else {
            return None;
        };
        if total_rows == 0 {
            return None;
        }
        Some(
            SelectionRange {
                start: (start.0, start.1.min(total_rows - 1)),
                end: (end.0, end.1.min(total_rows - 1)),
            }
            .normalized(),
        )
    }

    pub(super) fn reset_scrollback_view(&mut self) {
        self.session.scroll_offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::SelectionRange;

    #[test]
    fn selection_range_normalizes_descending_bounds() {
        let range = SelectionRange {
            start: (10, 8),
            end: (1, 3),
        }
        .normalized();
        assert_eq!(range.start, (1, 3));
        assert_eq!(range.end, (10, 8));
    }
}
