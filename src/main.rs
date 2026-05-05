use crate::ui::terminal_view::run;
mod pty;
mod terminal;
mod ui;

fn main() {
    if let Err(err) = run() {
        eprintln!("terminal failed to start: {err}");
        std::process::exit(1);
    }
}
