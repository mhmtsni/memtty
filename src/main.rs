use crate::ui::ui::MyApp;
mod pty;
mod terminal;
mod ui;

fn main() -> iced::Result {
    iced::application(MyApp::new, MyApp::update, MyApp::view)
        .subscription(MyApp::subscription)
        .run()
}
