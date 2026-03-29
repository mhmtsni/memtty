use crate::ui::terminal_view::run;
mod pty;
mod terminal;
mod ui;

#[tokio::main]
async fn main() {
    run().unwrap();
}
