use iced::mouse::Cursor;
use iced::widget::canvas::{self, Geometry, Path, Program};
use iced::{Color, Event, Rectangle, Renderer, Theme};

use crate::terminal::{Cell, CursorStyle, Terminal};
use crate::ui::ui::{Message, MyApp};

pub struct TerminalView<'a> {
    pub terminal: &'a Terminal,
    pub app_state: &'a MyApp,
}

impl<'a> Program<Message, Theme, Renderer> for TerminalView<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                let scroll_amount = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => *y as i32,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => (*y / 20.0) as i32,
                };
                if scroll_amount != 0 {
                    return Some(
                        canvas::Action::publish(Message::ScrollWheeled(-scroll_amount))
                            .and_capture(), // <-- event'i yakala, başka widget'a geçme
                    );
                }
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let char_width = 8.5;
        let char_height = 18.0;
        let visible_lines = (bounds.height / char_height) as usize;
        let offset = self.app_state.scroll_offset as usize;

        let scrollback = &self.terminal.performer.scrollback;
        let grid = &self.terminal.performer.grid;

        // Tüm satırları birleştir: scrollback + grid
        let all_rows: Vec<&Vec<Cell>> = scrollback.iter().chain(grid.iter()).collect();
        let total = all_rows.len();

        let end = total.saturating_sub(offset);
        let start = end.saturating_sub(visible_lines);

        for (y_screen, row) in all_rows[start..end].iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                if cell.c == ' ' && cell.bg == Color::BLACK {
                    continue;
                }
                if cell.bg != Color::BLACK {
                    frame.fill_rectangle(
                        iced::Point::new(x as f32 * char_width, y_screen as f32 * char_height),
                        iced::Size::new(char_width, char_height),
                        cell.bg,
                    );
                }
                frame.fill_text(canvas::Text {
                    content: cell.c.to_string(),
                    position: iced::Point::new(
                        x as f32 * char_width,
                        y_screen as f32 * char_height,
                    ),
                    color: cell.fg,
                    size: 14.0.into(),
                    font: iced::Font::MONOSPACE,
                    align_x: iced::widget::text::Alignment::Left,
                    align_y: iced::alignment::Vertical::Top,
                    ..Default::default()
                });
            }
        }
        let cursor_x = self.terminal.performer.cursor_x;
        let cursor_y = self.terminal.performer.cursor_y;
        let x = cursor_x as f32 * char_width;
        let y = cursor_y as f32 * char_height;
        let is_cursor_visible = self.terminal.performer.cursor_visible;
        match self.terminal.performer.cursor_style {
            CursorStyle::Block => {
                let rect = Path::rectangle(
                    iced::Point::new(x, y),
                    iced::Size::new(char_width, char_height),
                );

                if is_cursor_visible {
                    frame.fill(&rect, Color::from_rgba(1.0, 1.0, 1.0, 0.7));
                }
            }
            CursorStyle::Bar => {
                let rect = Path::rectangle(
                    iced::Point::new(x + char_width / 2.0 - 5.0, y),
                    iced::Size::new(1.0, char_height),
                );

                if is_cursor_visible {
                    frame.fill(&rect, Color::from_rgba(1.0, 1.0, 1.0, 1.0));
                }
            }
            CursorStyle::Underline => {
                let rect = Path::rectangle(
                    iced::Point::new(x, y + char_height - 3.0),
                    iced::Size::new(char_width, 2.0),
                );

                if is_cursor_visible {
                    frame.fill(&rect, Color::from_rgba(1.0, 1.0, 1.0, 1.0));
                }
            }
        }

        vec![frame.into_geometry()]
    }
}
