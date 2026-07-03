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
use crate::ui::ui::mouse::SelectionMode;
use crate::{
    pty::PtyInput,
    terminal::{CursorStyle, Terminal},
    ui::{
        renderer::{
            CursorRenderInfo, CursorRenderStyle, INDICATOR_WIDTH, Renderer,
            ScrollIndicatorRenderInfo, SettingsControlRenderKind, SettingsPanelRenderInfo,
            TAB_HEIGHT, TERMINAL_PADDING_X, TERMINAL_PADDING_Y, TabRenderInfo,
        },
        terminal_view::spawn_pty_for_tab,
    },
};

mod clipboard;
mod command_router;
mod core;
mod cursor;
mod history_completion;
mod input;
mod interaction_state;
mod mouse;
mod render_model;
mod render_sync;
mod resize;
mod scroll;
mod session_store;
mod settings;

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
    input_line: String,
    history_completion: Option<HistoryCompletionState>,
    history_preview: Option<String>,
    shell_history: Vec<String>,
}

impl Tab {
    pub fn new(id: usize, tx: Sender<PtyInput>) -> Self {
        Self {
            id,
            terminal: Terminal::new(),
            tx: Some(tx),
            pending_pty: Vec::new(),
            pending_pty_offset: 0,
            input_line: String::new(),
            history_completion: None,
            history_preview: None,
            shell_history: Vec::new(),
        }
    }
}

pub struct HistoryCompletionState {
    prefix: String,
    matches: Vec<String>,
    index: usize,
}

pub struct SessionStore {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub scroll_offset: i32,
}

impl SessionStore {
    pub fn new(initial_tab: Tab) -> Self {
        Self {
            tabs: vec![initial_tab],
            active_tab: 0,
            scroll_offset: 0,
        }
    }
}

pub struct InteractionState {
    pub mouse_position: PhysicalPosition<f64>,
    pub mouse_icon: CursorIcon,
    pub mouse_button_held: Option<MouseButton>,
    pub mouse_hold_start: Option<Instant>,
    pub left_press_position: Option<PhysicalPosition<f64>>,
    pub dragging_scroll_indicator: bool,
    pub drag_start_y: f64,
    pub drag_start_scroll_offset: i32,
    pub scroll_indicator_last_interaction: Option<Instant>,
    pub scroll_indicator_last_alpha: f32,
    pub selection_start: Option<(usize, usize)>,
    pub selection_end: Option<(usize, usize)>,
    pub selecting: bool,
    pub last_left_click_at: Option<Instant>,
    pub last_left_click_cell: Option<(usize, usize)>,
    pub left_click_streak: u8,
    pub selection_mode: SelectionMode,
    pub selection_anchor: Option<(usize, usize)>,
    pub settings_panel_open: bool,
    pub link_settings: LinkInteractionSettings,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            mouse_position: PhysicalPosition::new(-1000.0, -1000.0),
            mouse_icon: CursorIcon::Text,
            mouse_button_held: None,
            mouse_hold_start: None,
            left_press_position: None,
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
            selection_mode: SelectionMode::Char,
            selection_anchor: None,
            settings_panel_open: false,
            link_settings: LinkInteractionSettings::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct LinkInteractionSettings {
    pub enable_hyperlinks: bool,
    pub enable_plaintext_links: bool,
    pub enable_hover_underline: bool,
    pub enable_cmd_click_open: bool,
    pub disable_in_alt_screen: bool,
}

impl Default for LinkInteractionSettings {
    fn default() -> Self {
        Self {
            enable_hyperlinks: true,
            enable_plaintext_links: true,
            enable_hover_underline: true,
            enable_cmd_click_open: true,
            disable_in_alt_screen: true,
        }
    }
}

pub struct MyApp {
    window: Arc<Window>,
    pub session: SessionStore,
    pub interaction: InteractionState,
    full_screen: bool,
    modifiers: ModifiersState,
    pub renderer: Renderer,
    cursor_blink_on: bool,
    last_blink: Instant,
    pub has_focus: bool,
}

impl MyApp {
    pub fn new(window: Arc<Window>, tx_to_pty: Sender<PtyInput>, renderer: Renderer) -> Self {
        let mut app = Self {
            full_screen: false,
            session: SessionStore::new(Tab::new(0, tx_to_pty)),
            interaction: InteractionState::default(),
            window: window.clone(),
            modifiers: ModifiersState::empty(),
            renderer,
            cursor_blink_on: true,
            last_blink: std::time::Instant::now(),
            has_focus: true,
        };

        app.sync_renderer_from_terminal(true);
        app
    }
}
