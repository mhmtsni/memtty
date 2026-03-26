use std::{fmt::Debug, time::Duration};

use iced::{
    Color, Subscription, event,
    keyboard::{Event, Key, key::Named},
    time,
    widget::{Canvas, container},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    pty::{PtyInput, run},
    terminal::Terminal,
    ui::terminal_view::TerminalView,
};

#[derive(Clone, Debug)]
pub enum Message {
    PtyDataReceived(Vec<u8>),
    PtyExited,
    EventOccured(event::Event),
    ScrollWheeled(i32),
    Tick,
    Paste(String),
}

#[derive(Default)]
pub struct MyApp {
    tx: Option<mpsc::Sender<PtyInput>>,
    terminal: Terminal,
    pub scroll_offset: i32,
    full_screen: bool,
}

const BLINKING_INTERVAL: u64 = 500;

impl MyApp {
    pub fn new() -> (Self, iced::Task<Message>) {
        let (tx_to_pty, rx_from_ui) = mpsc::channel::<PtyInput>(100);
        let (tx_to_ui, rx_from_pty) = mpsc::channel::<Vec<u8>>(100);

        let start_pty = iced::Task::perform(
            async move {
                let _ = run(tx_to_ui, rx_from_ui).await;
            },
            |_| Message::PtyExited,
        );

        // Stream PTY output into the app message loop.
        let read_pty_output =
            iced::Task::run(ReceiverStream::new(rx_from_pty), Message::PtyDataReceived);

        let task = iced::Task::batch([start_pty, read_pty_output]);

        (
            Self {
                full_screen: false,
                tx: Some(tx_to_pty),
                terminal: Terminal::new(),
                scroll_offset: 0,
            },
            task,
        )
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PtyDataReceived(data) => {
                self.terminal.process(&data);
                iced::Task::none().into()
            }
            Message::PtyExited => {
                std::process::exit(0);
            }
            Message::EventOccured(event) => {
                match event {
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        modified_key,
                        physical_key,
                        location,
                        text,
                        repeat,
                    }) => {
                        if matches!(key, iced::keyboard::Key::Named(Named::Enter))
                            && modifiers.command()
                        {
                            self.full_screen = !self.full_screen;
                            let mode = if self.full_screen {
                                iced::window::Mode::Fullscreen
                            } else {
                                iced::window::Mode::Windowed
                            };
                            return iced::window::latest()
                                .then(move |id| iced::window::set_mode(id.unwrap(), mode));
                        }

                        if matches!(key, iced::keyboard::Key::Character(ref c) if c.to_lowercase() == "v")
                            && modifiers.command()
                        {
                            return iced::clipboard::read().map(|content| {
                                if let Some(text) = content {
                                    Message::Paste(text)
                                } else {
                                    Message::Paste(String::new())
                                }
                            });
                        }

                        // Cmd+Enter değilse handle_key'e gönder
                        self.handle_key(iced::keyboard::Event::KeyPressed {
                            key,
                            modifiers,
                            modified_key,
                            physical_key,
                            location,
                            text,
                            repeat,
                        });
                        iced::Task::none()
                    }

                    // Fix: Use tuple pattern matching (width, height)
                    iced::Event::Window(iced::window::Event::Resized(size)) => {
                        let width = size.width;
                        let height = size.height;

                        // Calculation based on Monospace Font 14:
                        // Width: ~8.5px per char, Height: ~18px per char
                        let new_cols = (width / 8.5).max(10.0) as u16;
                        let new_rows = (height / 18.0).max(5.0) as u16;

                        // 1. Update Internal Grid
                        self.terminal
                            .performer
                            .resize(new_cols as usize, new_rows as usize);

                        // 2. Notify PTY
                        self.send_to_pty(PtyInput::Resize {
                            cols: new_cols,
                            rows: new_rows,
                        });
                        iced::Task::none().into()
                    }

                    _ => iced::Task::none().into(),
                }
            }

            Message::ScrollWheeled(delta) => {
                self.scroll_offset = (self.scroll_offset - delta)
                    .max(0)
                    .min(self.terminal.performer.grid.len() as i32 - 1);
                if self.scroll_offset > 0 {
                    self.terminal.performer.cursor_visible = false;
                } else {
                    self.terminal.performer.cursor_visible = true;
                }

                iced::Task::none().into()
            }
            Message::Tick => {
                if self.terminal.performer.cursor_blinking {
                    self.terminal.performer.cursor_visible =
                        !self.terminal.performer.cursor_visible;
                }
                iced::Task::none().into()
            }
            Message::Paste(text) => {
                let mut data = Vec::new();

                data.extend_from_slice(b"\x1b[200~");

                let normalized = text.replace("\n", "\r");
                data.extend_from_slice(normalized.as_bytes());

                data.extend_from_slice(b"\x1b[201~");

                self.send_to_pty(PtyInput::Data(data));

                iced::Task::none()
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            event::listen().map(Message::EventOccured),
            time::every(Duration::from_millis(BLINKING_INTERVAL)).map(|_| Message::Tick),
        ])
    }

    fn send_to_pty(&mut self, data: PtyInput) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(data);
        }
    }

    fn handle_key(&mut self, event: Event) {
        if let Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        } = event
        {
            let bytes: Vec<u8> = match key {
                // -------------------------------------------------------
                // Ctrl + Arrow (önce, çünkü daha spesifik)
                // -------------------------------------------------------
                Key::Named(Named::ArrowUp) if modifiers.alt() => b"\x1b[1;5A".to_vec(),
                Key::Named(Named::ArrowDown) if modifiers.alt() => b"\x1b[1;5B".to_vec(),
                Key::Named(Named::ArrowRight) if modifiers.alt() => b"\x1bf".to_vec(),
                Key::Named(Named::ArrowLeft) if modifiers.alt() => b"\x1bb".to_vec(),
                Key::Named(Named::Backspace) if modifiers.alt() => b"\x1b\x7f".to_vec(), // Alt+Backspace → kelime sil

                // -------------------------------------------------------
                // Ctrl kombinasyonları (önce, çünkü daha spesifik)
                // -------------------------------------------------------
                Key::Character(ref c) if modifiers.control() => match c.to_lowercase().as_str() {
                    "a" => b"\x01".to_vec(),
                    "b" => b"\x02".to_vec(),
                    "c" => b"\x03".to_vec(),
                    "d" => b"\x04".to_vec(),
                    "e" => b"\x05".to_vec(),
                    "f" => b"\x06".to_vec(),
                    "g" => b"\x07".to_vec(),
                    "h" => b"\x08".to_vec(),
                    "i" => b"\x09".to_vec(),
                    "j" => b"\x0a".to_vec(),
                    "k" => b"\x0b".to_vec(),
                    "l" => b"\x0c".to_vec(),
                    "m" => b"\x0d".to_vec(),
                    "n" => b"\x0e".to_vec(),
                    "o" => b"\x0f".to_vec(),
                    "p" => b"\x10".to_vec(),
                    "q" => b"\x11".to_vec(),
                    "r" => b"\x12".to_vec(),
                    "s" => b"\x13".to_vec(),
                    "t" => b"\x14".to_vec(),
                    "u" => b"\x15".to_vec(),
                    "v" => b"\x16".to_vec(),
                    "w" => b"\x17".to_vec(),
                    "x" => b"\x18".to_vec(),
                    "y" => b"\x19".to_vec(),
                    "z" => b"\x1a".to_vec(),
                    "[" => b"\x1b".to_vec(),
                    "\\" => b"\x1c".to_vec(),
                    "]" => b"\x1d".to_vec(),
                    _ => return,
                },

                // -------------------------------------------------------
                // Backspace + Cmd → satırı sil
                // -------------------------------------------------------
                Key::Named(Named::Backspace) if modifiers.command() => b"\x15".to_vec(),

                // -------------------------------------------------------
                // Shift + Tab
                // -------------------------------------------------------
                Key::Named(Named::Tab) if modifiers.shift() => b"\x1b[Z".to_vec(),

                // -------------------------------------------------------
                // Temel tuşlar
                // -------------------------------------------------------
                Key::Named(Named::Enter) => b"\r".to_vec(),
                Key::Named(Named::Backspace) => b"\x7f".to_vec(),
                Key::Named(Named::Escape) => b"\x1b".to_vec(),
                Key::Named(Named::Tab) => b"\t".to_vec(),
                Key::Named(Named::Space) => b" ".to_vec(),
                Key::Named(Named::Delete) => b"\x1b[3~".to_vec(),

                // -------------------------------------------------------
                // Ok tuşları
                // -------------------------------------------------------
                Key::Named(Named::ArrowUp) => b"\x1b[A".to_vec(),
                Key::Named(Named::ArrowDown) => b"\x1b[B".to_vec(),
                Key::Named(Named::ArrowRight) => b"\x1b[C".to_vec(),
                Key::Named(Named::ArrowLeft) => b"\x1b[D".to_vec(),

                // -------------------------------------------------------
                // Navigation
                // -------------------------------------------------------
                Key::Named(Named::Home) => b"\x1b[H".to_vec(),
                Key::Named(Named::End) => b"\x1b[F".to_vec(),
                Key::Named(Named::PageUp) => b"\x1b[5~".to_vec(),
                Key::Named(Named::PageDown) => b"\x1b[6~".to_vec(),

                // -------------------------------------------------------
                // F1-F12
                // -------------------------------------------------------
                Key::Named(Named::F1) => b"\x1bOP".to_vec(),
                Key::Named(Named::F2) => b"\x1bOQ".to_vec(),
                Key::Named(Named::F3) => b"\x1bOR".to_vec(),
                Key::Named(Named::F4) => b"\x1bOS".to_vec(),
                Key::Named(Named::F5) => b"\x1b[15~".to_vec(),
                Key::Named(Named::F6) => b"\x1b[17~".to_vec(),
                Key::Named(Named::F7) => b"\x1b[18~".to_vec(),
                Key::Named(Named::F8) => b"\x1b[19~".to_vec(),
                Key::Named(Named::F9) => b"\x1b[20~".to_vec(),
                Key::Named(Named::F10) => b"\x1b[21~".to_vec(),
                Key::Named(Named::F11) => b"\x1b[23~".to_vec(),
                Key::Named(Named::F12) => b"\x1b[24~".to_vec(),

                // -------------------------------------------------------
                // Normal karakter
                // -------------------------------------------------------
                _ => {
                    if let Some(text) = text {
                        self.scroll_offset = 0;
                        self.terminal.performer.cursor_visible = true;
                        return self.send_to_pty(PtyInput::Data(text.as_bytes().to_vec()));
                    }

                    self.scroll_offset = 0;
                    return;
                }
            };

            self.send_to_pty(PtyInput::Data(bytes));

            self.scroll_offset = 0;
            self.terminal.performer.cursor_visible = true;
        }
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        container(
            Canvas::new(TerminalView {
                terminal: &self.terminal,
                app_state: self,
            })
            .width(iced::Fill)
            .height(iced::Fill),
        )
        .width(iced::Fill)
        .height(iced::Fill)
        .padding(5)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(Color::BLACK)),
            ..Default::default()
        })
        .into()
    }
}
