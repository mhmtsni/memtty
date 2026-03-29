use std::sync::Arc;

use crate::ui::ui::Message;
use winit::{
    application::ApplicationHandler,
    event::{KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::PhysicalKey,
    window::Window,
};

use crate::ui::ui::MyApp;

pub struct TerminalView {
    // pub terminal: Terminal,
    pub app: Option<MyApp>,
    pub window: Option<Arc<Window>>,
    pub proxy: Option<EventLoopProxy<Message>>,
}

// impl<'a> Program<Message, Theme, Renderer> for TerminalView<'a> {
//     type State = ();
//
//     fn update(
//         &self,
//         _state: &mut Self::State,
//         event: &iced::Event,
//         _bounds: Rectangle,
//         _cursor: Cursor,
//     ) -> Option<canvas::Action<Message>> {
//         match event {
//             Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
//                 let scroll_amount = match delta {
//                     iced::mouse::ScrollDelta::Lines { y, .. } => *y as i32,
//                     iced::mouse::ScrollDelta::Pixels { y, .. } => (*y / 20.0) as i32,
//                 };
//                 if scroll_amount != 0 {
//                     return Some(
//                         canvas::Action::publish(Message::ScrollWheeled(-scroll_amount))
//                             .and_capture(), // <-- event'i yakala, başka widget'a geçme
//                     );
//                 }
//                 Some(canvas::Action::capture())
//             }
//             _ => None,
//         }
//     }
//
//     fn draw(
//         &self,
//         _state: &Self::State,
//         renderer: &Renderer,
//         _theme: &Theme,
//         bounds: Rectangle,
//         _cursor: Cursor,
//     ) -> Vec<Geometry> {
//         let mut frame = canvas::Frame::new(renderer, bounds.size());
//         let char_width = 8.5;
//         let char_height = 18.0;
//         let visible_lines = (bounds.height / char_height) as usize;
//
//         let scrollback = &self.terminal.performer.scrollback;
//         let grid = &self.terminal.performer.grid;
//         let scrollback_len = scrollback.len();
//         let total_lines = scrollback_len + grid.len();
//         let max_offset = total_lines.saturating_sub(visible_lines);
//         let offset = (self.app_state.scroll_offset as usize).min(max_offset);
//
//         let end = total_lines.saturating_sub(offset);
//         let start = end.saturating_sub(visible_lines);
//
//         for (y_screen, line_index) in (start..end).enumerate() {
//             let row = if line_index < scrollback_len {
//                 &scrollback[line_index]
//             } else {
//                 &grid[line_index - scrollback_len]
//             };
//
//             for (x, cell) in row.iter().enumerate() {
//                 if cell.c == ' ' && cell.bg == Color::BLACK {
//                     continue;
//                 }
//                 if cell.bg != Color::BLACK {
//                     frame.fill_rectangle(
//                         iced::Point::new(x as f32 * char_width, y_screen as f32 * char_height),
//                         iced::Size::new(char_width, char_height),
//                         cell.bg,
//                     );
//                 }
//                 frame.fill_text(canvas::Text {
//                     content: cell.c.to_string(),
//                     position: iced::Point::new(
//                         x as f32 * char_width,
//                         y_screen as f32 * char_height,
//                     ),
//                     color: cell.fg,
//                     size: 14.0.into(),
//                     font: iced::Font::MONOSPACE,
//                     align_x: iced::widget::text::Alignment::Left,
//                     align_y: iced::alignment::Vertical::Top,
//                     ..Default::default()
//                 });
//             }
//         }
//         let cursor_x = self.terminal.performer.cursor_x;
//         let cursor_y = self.terminal.performer.cursor_y;
//         let x = cursor_x as f32 * char_width;
//         let y = cursor_y as f32 * char_height;
//         let is_cursor_visible = self.terminal.performer.cursor_visible;
//         match self.terminal.performer.cursor_style {
//             CursorStyle::Block => {
//                 let rect = Path::rectangle(
//                     iced::Point::new(x, y),
//                     iced::Size::new(char_width, char_height),
//                 );
//
//                 if is_cursor_visible {
//                     frame.fill(&rect, Color::from_rgba(1.0, 1.0, 1.0, 0.7));
//                 }
//             }
//             CursorStyle::Bar => {
//                 let rect = Path::rectangle(
//                     iced::Point::new(x + char_width / 2.0 - 5.0, y),
//                     iced::Size::new(1.0, char_height),
//                 );
//
//                 if is_cursor_visible {
//                     frame.fill(&rect, Color::from_rgba(1.0, 1.0, 1.0, 1.0));
//                 }
//             }
//             CursorStyle::Underline => {
//                 let rect = Path::rectangle(
//                     iced::Point::new(x, y + char_height - 3.0),
//                     iced::Size::new(char_width, 2.0),
//                 );
//
//                 if is_cursor_visible {
//                     frame.fill(&rect, Color::from_rgba(1.0, 1.0, 1.0, 1.0));
//                 }
//             }
//         }
//
//         vec![frame.into_geometry()]
//     }
// }
//

impl ApplicationHandler<Message> for TerminalView {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("Terminal"))
                .unwrap(),
        );

        // channels
        let (tx_to_pty, rx_from_ui) = tokio::sync::mpsc::channel(100);
        let (tx_to_ui, mut rx_from_pty) = tokio::sync::mpsc::channel(100);

        // create app
        let app = MyApp::new(window.clone(), tx_to_pty);

        let proxy = self.proxy.as_ref().unwrap().clone();

        // spawn PTY
        tokio::spawn(async move {
            tokio::spawn(async move {
                let _ = crate::pty::run(tx_to_ui, rx_from_ui).await;
            });

            while let Some(data) = rx_from_pty.recv().await {
                let _ = proxy.send_event(Message::PtyDataReceived(data));
            }

            let _ = proxy.send_event(Message::PtyExited);
        });

        self.window = Some(window);
        self.app = Some(app);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Message) {
        let app = match &mut self.app {
            Some(app) => app,
            None => return,
        };

        match event {
            Message::PtyDataReceived(data) => {
                println!("PTY DATA: {} bytes", data.len());
                app.terminal.process(&data);
            }
            Message::PtyExited => {
                std::process::exit(0);
            }
            _ => {}
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = match &mut self.app {
            Some(canvas) => canvas,
            None => return,
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                println!("REDRAW CALLED");

                for row in &state.terminal.performer.grid {
                    let line: String = row.iter().map(|c| c.c).collect();
                    println!("{}", line);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: key_state,
                        ..
                    },
                ..
            } => match (code, key_state.is_pressed()) {
                _ => {}
            },
            _ => {}
        }
    }
    // ...
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let mut app = TerminalView {
        app: None,
        window: None,
        proxy: Some(proxy),
    };

    event_loop.run_app(&mut app)?;

    Ok(())
}
