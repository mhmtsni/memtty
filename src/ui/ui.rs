use std::sync::mpsc::Sender;
use std::{fmt::Debug, sync::Arc};

use arboard::Clipboard;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta},
    event_loop::EventLoopProxy,
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Fullscreen, Window},
};

use crate::terminal::performer::MouseMode;
use crate::{
    pty::PtyInput,
    terminal::{CursorStyle, Terminal},
    ui::{
        renderer::{
            CursorRenderInfo, CursorRenderStyle, INDICATOR_WIDTH, Renderer,
            ScrollIndicatorRenderInfo, TAB_HEIGHT, TERMINAL_PADDING_X, TERMINAL_PADDING_Y,
            TabRenderInfo,
        },
        terminal_view::spawn_pty_for_tab,
    },
};

mod clipboard;
mod core;
mod cursor;
mod input;
mod mouse;
mod render_sync;
mod resize;
mod scroll;

#[derive(Clone, Debug)]
pub enum Message {
    PtyDataReceived(usize, Vec<u8>),
    PtyExited(usize),
    Exit,
}

use std::time::{Duration, Instant};

const CURSOR_COLOR: glyphon::Color = glyphon::Color::rgb(255, 255, 255);

pub struct Tab {
    pub id: usize,
    pub terminal: Terminal,
    tx: Option<Sender<PtyInput>>,
    pending_pty: Vec<u8>,
    pending_pty_offset: usize,
}

pub struct MyApp {
    window: Arc<Window>,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub scroll_offset: i32,
    pub mouse_position: PhysicalPosition<f64>,
    pub mouse_icon: CursorIcon,

    mouse_button_held: Option<MouseButton>,
    mouse_hold_start: Option<Instant>,
    full_screen: bool,
    modifiers: ModifiersState,
    pub renderer: Renderer,
    cursor_blink_on: bool,
    last_blink: Instant,
    pub has_focus: bool,
    dragging_scroll_indicator: bool,
    drag_start_y: f64,
    drag_start_scroll_offset: i32,
    scroll_indicator_last_interaction: Option<Instant>,
    scroll_indicator_last_alpha: f32,
    selection_start: Option<(usize, usize)>,
    selection_end: Option<(usize, usize)>,
    selecting: bool,
    last_left_click_at: Option<Instant>,
    last_left_click_cell: Option<(usize, usize)>,
    left_click_streak: u8,
}

impl MyApp {
    pub fn new(window: Arc<Window>, tx_to_pty: Sender<PtyInput>, renderer: Renderer) -> Self {
        let mut app = Self {
            full_screen: false,
            mouse_icon: CursorIcon::Text,
            tabs: vec![Tab {
                id: 0,
                terminal: Terminal::new(),
                tx: Some(tx_to_pty),
                pending_pty: Vec::new(),
                pending_pty_offset: 0,
            }],
            active_tab: 0,
            mouse_position: PhysicalPosition::new(0.0, 0.0),
            scroll_offset: 0,
            window,
            modifiers: ModifiersState::empty(),
            renderer,
            cursor_blink_on: true,
            last_blink: std::time::Instant::now(),
            has_focus: true,
            mouse_button_held: None,
            mouse_hold_start: None,
            dragging_scroll_indicator: false,
            drag_start_y: 0.0,
            drag_start_scroll_offset: 0,
            scroll_indicator_last_interaction: None,
            scroll_indicator_last_alpha: 0.0,
            selection_start: None,
            selection_end: None,
            selecting: false,
            last_left_click_at: None,
            last_left_click_cell: None,
            left_click_streak: 0,
        };

        app.sync_renderer_from_terminal(true);
        app
    }
}
