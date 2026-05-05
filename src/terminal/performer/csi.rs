use vte::Params;

use super::{CursorStyle, Performer};

impl Performer {
    pub(super) fn dispatch_csi(&mut self, params: &Params, intermediates: &[u8], action: char) {
        let params_vec: Vec<u16> = params
            .iter()
            .map(|sub| sub.first().copied().unwrap_or(0))
            .collect();

        let p = |idx: usize| params_vec.get(idx).copied().unwrap_or(0) as usize;
        let p1 = |idx: usize| p(idx).max(1);

        if intermediates.first() == Some(&b'?') {
            self.dispatch_private_csi(&params_vec, action);
            return;
        }

        if intermediates.first() == Some(&b'>') {
            if action == 'c' {
                self.queue_pty_reply(b"\x1b[>0;0;0c".to_vec());
            }
            return;
        }

        match action {
            'A' => {
                let n = p1(0);
                self.cursor_y = self.cursor_y.saturating_sub(n).max(self.scroll_top);
                self.pending_wrap = false;
            }
            'B' => {
                let n = p1(0);
                self.cursor_y = (self.cursor_y + n).min(self.scroll_bottom);
                self.pending_wrap = false;
            }
            'C' => {
                let n = p1(0);
                self.cursor_x = (self.cursor_x + n).min(self.cols - 1);
                self.pending_wrap = false;
            }
            'D' => {
                let n = p1(0);
                self.cursor_x = self.cursor_x.saturating_sub(n);
                self.pending_wrap = false;
            }
            'E' => {
                let n = p1(0);
                self.cursor_y = (self.cursor_y + n).min(self.rows - 1);
                self.cursor_x = 0;
                self.pending_wrap = false;
            }
            'F' => {
                let n = p1(0);
                self.cursor_y = self.cursor_y.saturating_sub(n);
                self.cursor_x = 0;
                self.pending_wrap = false;
            }
            'G' | '`' => {
                self.cursor_x = (p1(0) - 1).min(self.cols - 1);
                self.pending_wrap = false;
            }
            'H' | 'f' => {
                let row = (p1(0) - 1).min(self.rows - 1);
                let col = (p1(1) - 1).min(self.cols - 1);
                self.cursor_y = if self.origin_mode {
                    (self.scroll_top + row).min(self.scroll_bottom)
                } else {
                    row
                };
                self.cursor_x = col;
                self.pending_wrap = false;
            }
            'd' => {
                let row = (p1(0) - 1).min(self.rows - 1);
                self.cursor_y = if self.origin_mode {
                    (self.scroll_top + row).min(self.scroll_bottom)
                } else {
                    row
                };
                self.pending_wrap = false;
            }
            'J' => self.erase_in_display(p(0)),
            'K' => self.erase_in_line(p(0)),
            'X' => self.erase_characters(p1(0)),
            'S' => self.scroll_up_region(p1(0)),
            'T' => self.scroll_down_region(p1(0)),
            'L' => self.insert_lines(p1(0)),
            'M' => self.delete_lines(p1(0)),
            'P' => self.delete_characters(p1(0)),
            '@' => self.insert_blank_characters(p1(0)),
            'b' => self.repeat_last_printed(p1(0)),
            'q' if intermediates.first() == Some(&b' ') => self.set_cursor_style(p(0)),
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),
            'r' => self.set_scroll_region(p1(0), p(1)),
            'n' => self.report_device_status(&params_vec, false),
            'c' => {
                self.queue_pty_reply(b"\x1b[?1;2c".to_vec());
            }
            'm' => self.apply_sgr(params),
            'h' => self.set_public_modes(&params_vec, true),
            'l' => self.set_public_modes(&params_vec, false),
            _ => {}
        }
    }

    fn dispatch_private_csi(&mut self, params_vec: &[u16], action: char) {
        match action {
            'h' => {
                for &mode in params_vec {
                    self.set_dec_mode(mode as usize, true);
                }
            }
            'l' => {
                for &mode in params_vec {
                    self.set_dec_mode(mode as usize, false);
                }
            }
            'n' => self.report_device_status(params_vec, true),
            _ => {}
        }
    }

    fn erase_in_display(&mut self, mode: usize) {
        let empty = self.empty_cell();
        match mode {
            0 => {
                if self.cursor_y < self.grid.len() {
                    let row_len = self.grid[self.cursor_y].len();
                    for x in self.cursor_x..row_len {
                        self.grid[self.cursor_y][x] = empty.clone();
                    }
                }
                for y in (self.cursor_y + 1)..self.grid.len() {
                    self.grid[y] = self.empty_row();
                }
            }
            1 => {
                for y in 0..self.cursor_y.min(self.grid.len()) {
                    self.grid[y] = self.empty_row();
                }
                if self.cursor_y < self.grid.len() {
                    let end = (self.cursor_x + 1).min(self.grid[self.cursor_y].len());
                    for x in 0..end {
                        self.grid[self.cursor_y][x] = empty.clone();
                    }
                }
            }
            2 | 3 => {
                if mode == 3 {
                    self.scrollback.clear();
                }
                let blank_row = self.empty_row();
                for row in &mut self.grid {
                    *row = blank_row.clone();
                }
                self.cursor_x = 0;
                self.cursor_y = 0;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: usize) {
        let empty = self.empty_cell();
        if self.cursor_y >= self.grid.len() {
            return;
        }

        match mode {
            0 => {
                for x in self.cursor_x..self.cols {
                    self.grid[self.cursor_y][x] = empty.clone();
                }
            }
            1 => {
                for x in 0..=self.cursor_x.min(self.cols - 1) {
                    self.grid[self.cursor_y][x] = empty.clone();
                }
            }
            2 => self.grid[self.cursor_y] = self.empty_row(),
            _ => {}
        }
    }

    fn erase_characters(&mut self, count: usize) {
        let empty = self.empty_cell();
        for x in self.cursor_x..(self.cursor_x + count).min(self.cols) {
            self.grid[self.cursor_y][x] = empty.clone();
        }
    }

    fn insert_lines(&mut self, count: usize) {
        if self.cursor_y >= self.scroll_top && self.cursor_y <= self.scroll_bottom {
            for _ in 0..count {
                if self.scroll_bottom < self.grid.len() {
                    self.grid.remove(self.scroll_bottom);
                }
                self.grid.insert(self.cursor_y, self.empty_row());
            }
        }
        self.cursor_x = 0;
        self.pending_wrap = false;
    }

    fn delete_lines(&mut self, count: usize) {
        for _ in 0..count {
            if self.cursor_y < self.grid.len() {
                self.grid.remove(self.cursor_y);
            }
            let ins = (self.scroll_bottom + 1).min(self.grid.len());
            self.grid.insert(ins, self.empty_row());
        }
        self.cursor_x = 0;
        self.pending_wrap = false;
    }

    fn delete_characters(&mut self, count: usize) {
        let empty = self.empty_cell();
        if self.cursor_y < self.grid.len() {
            let row = &mut self.grid[self.cursor_y];
            for _ in 0..count {
                if self.cursor_x < row.len() {
                    row.remove(self.cursor_x);
                    row.push(empty.clone());
                }
            }
        }
    }

    fn insert_blank_characters(&mut self, count: usize) {
        let empty = self.empty_cell();
        if self.cursor_y < self.grid.len() {
            let row = &mut self.grid[self.cursor_y];
            for _ in 0..count {
                if row.len() >= self.cols {
                    row.pop();
                }
                row.insert(self.cursor_x, empty.clone());
            }
        }
    }

    fn repeat_last_printed(&mut self, count: usize) {
        if let Some(last) = self.last_printed {
            for _ in 0..count {
                self.print_char(last);
            }
        }
    }

    fn set_cursor_style(&mut self, mode: usize) {
        match mode {
            0 | 1 | 2 => self.cursor_style = CursorStyle::Block,
            3 | 4 => self.cursor_style = CursorStyle::Underline,
            5 | 6 => self.cursor_style = CursorStyle::Bar,
            _ => {}
        }
        self.cursor_blinking = matches!(mode, 0 | 1 | 3 | 5);
    }

    fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let top = top.saturating_sub(1);
        let bottom = if bottom == 0 {
            self.rows - 1
        } else {
            bottom - 1
        };
        if top < bottom && bottom < self.rows {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        }
        self.cursor_x = 0;
        self.cursor_y = if self.origin_mode { self.scroll_top } else { 0 };
        self.pending_wrap = false;
    }

    fn report_device_status(&mut self, params_vec: &[u16], private: bool) {
        for &code in params_vec {
            match code {
                5 => {
                    let reply = if private {
                        b"\x1b[?0n".to_vec()
                    } else {
                        b"\x1b[0n".to_vec()
                    };
                    self.queue_pty_reply(reply);
                }
                6 => {
                    let row = self.cursor_y + 1;
                    let col = self.cursor_x + 1;
                    let reply = if private {
                        format!("\x1b[?{};{}R", row, col)
                    } else {
                        format!("\x1b[{};{}R", row, col)
                    };
                    self.queue_pty_reply(reply.into_bytes());
                }
                _ => {}
            }
        }
    }

    fn set_public_modes(&mut self, params_vec: &[u16], enable: bool) {
        for &mode in params_vec {
            if mode == 4 {
                self.insert_mode = enable;
            }
        }
    }
}
