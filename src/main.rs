use crate::ui::terminal_view::run;
mod pty;
mod terminal;
mod ui;

fn main() {
    run().unwrap();
}
