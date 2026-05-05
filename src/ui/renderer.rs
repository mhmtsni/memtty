use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Weight, cosmic_text::Align,
};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

use crate::terminal::{Cell, style};

mod renderer_background;
mod renderer_cache;
mod renderer_cursor;
mod renderer_dirty;
mod renderer_scroll_indicator;
mod renderer_tab;
mod renderer_text;

const LINE_HEIGHT_FACTOR: f32 = 1.2;
const CELL_WIDTH_FACTOR: f32 = 0.55;
const INITIAL_SOLID_VERTEX_CAPACITY: usize = 2048;
const CELL_WIDTH_SAMPLE_COUNT: usize = 32;
const UI_FONT_SIZE: f32 = 14.0;
const UI_LINE_HEIGHT: f32 = 18.0;
const UI_TITLE_FONT_SIZE: f32 = 21.0;
const UI_TITLE_LINE_HEIGHT: f32 = 28.0;
const UI_DETAIL_FONT_SIZE: f32 = 12.0;
const UI_DETAIL_LINE_HEIGHT: f32 = 16.0;
const UI_VALUE_FONT_SIZE: f32 = 13.0;
const UI_VALUE_LINE_HEIGHT: f32 = 17.0;
pub const TAB_HEIGHT: usize = 70;
pub const INDICATOR_WIDTH: f32 = 12.0;
pub const TERMINAL_PADDING_X: f32 = 8.0;
pub const TERMINAL_PADDING_Y: f32 = 1.0;

const SOLID_SHADER: &str = r#"
struct VsOut {
    @builtin(position) position : vec4<f32>,
    @location(0) color : vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[derive(Clone, Copy)]
pub enum CursorRenderStyle {
    Block,
    Underline,
    Bar,
    Unfocused,
}

#[derive(Clone, Copy)]
pub struct CursorRenderInfo {
    pub col: usize,
    pub row: usize,
    pub style: CursorRenderStyle,
    pub color: Color,
    pub blink_on: bool,
}

#[derive(Clone)]
pub struct TabRenderInfo {
    pub title: String,
    pub is_hovered: bool,
    pub active: bool,
    pub width: usize,
    pub height: usize,
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy)]
pub struct UiRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone)]
pub struct SettingsItemRenderInfo {
    pub title: String,
    pub detail: String,
    pub value: String,
    pub control: SettingsControlRenderKind,
    pub is_hovered: bool,
    pub rect: UiRect,
    pub primary_rect: UiRect,
    pub secondary_rect: Option<UiRect>,
}

#[derive(Clone, Copy)]
pub enum SettingsControlRenderKind {
    Menu,
    Toggle { enabled: bool },
    Stepper,
    Button,
}

#[derive(Clone)]
pub struct SettingsPanelRenderInfo {
    pub is_open: bool,
    pub button_rect: UiRect,
    pub button_hovered: bool,
    pub panel_rect: UiRect,
    pub sidebar_rect: UiRect,
    pub content_rect: UiRect,
    pub items: Vec<SettingsItemRenderInfo>,
}

#[derive(Clone)]
pub struct ScrollIndicatorRenderInfo {
    pub visible: bool,
    pub opacity: f32,
    pub position_y: f32,
    pub is_mouse_on_indicator: bool,
    pub height: f32,
    pub in_alt_screen: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SolidVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl SolidVertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SolidVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS,
        }
    }
}

/// A cheaply comparable snapshot of a single cell — used for dirty detection.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellKey {
    c: char,
    wide_continuation: bool,
    text_hash: u64,
    hyperlink_hash: u64,
    is_link_hovered: bool,
    fg: u32,
    bg: u32,
    style: u8,
}

impl CellKey {
    fn from_cell(cell: &Cell) -> Self {
        let mut text_hasher = std::collections::hash_map::DefaultHasher::new();
        cell.text.hash(&mut text_hasher);

        let mut hyperlink_hasher = std::collections::hash_map::DefaultHasher::new();
        cell.hyperlink.hash(&mut hyperlink_hasher);

        Self {
            c: cell.c,
            wide_continuation: cell.wide_continuation,
            text_hash: text_hasher.finish(),
            hyperlink_hash: hyperlink_hasher.finish(),
            is_link_hovered: cell.is_link_hovered,
            fg: cell.fg.0,
            bg: cell.bg.0,
            style: cell.style,
        }
    }
}

pub struct Renderer {
    font_system: FontSystem,
    font_family_name: Option<&'static str>,
    available_font_families: Vec<FontFamilyOption>,
    font_family_index: usize,
    initial_font_family_index: usize,
    ligatures_enabled: bool,
    initial_ligatures_enabled: bool,
    swash_cache: SwashCache,
    buffer: Buffer,
    pub atlas: TextAtlas,
    pub text_renderer: TextRenderer,
    viewport: Viewport,
    text_format: TextureFormat,
    text_multisample: MultisampleState,
    pub width: u32,
    pub height: u32,
    pub line_height: f32,
    pub cell_width: f32,
    tab_buffer: Vec<Buffer>,
    tabs_cache: Vec<TabRenderInfo>,
    tabs_need_shape: bool,
    settings_button_buffer: Buffer,
    settings_title_buffer: Buffer,
    settings_sidebar_buffers: Vec<Buffer>,
    settings_item_buffers: Vec<Buffer>,
    settings_detail_buffers: Vec<Buffer>,
    settings_value_buffers: Vec<Buffer>,
    settings_cache: Option<SettingsPanelRenderInfo>,
    settings_need_shape: bool,
    spans_cache: Vec<(String, Attrs<'static>)>,
    solid_vertices: Vec<SolidVertex>,
    background_vertex_count: usize,
    solid_vertex_capacity: usize,
    solid_vertex_buffer: wgpu::Buffer,
    solid_pipeline: wgpu::RenderPipeline,
    pub font_size: f32,
    initial_font_size: f32,
    line_height_factor: f32,
    initial_line_height_factor: f32,
    needs_shape: bool,
    /// Snapshot of the last rendered grid — row × col cell keys.
    /// Used to skip rebuilding spans/geometry for unchanged rows.
    last_grid: Vec<Vec<CellKey>>,
    /// Cursor signature from the previous frame. If any visual aspect changes
    /// (position/style/color/blink visibility), we must redirty affected rows.
    last_cursor: Option<CursorCacheKey>,
    /// Whether the full text buffer needs to be rebuilt from scratch.
    /// Set when font size changes or the row count changes.
    full_rebuild: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CursorCacheKey {
    col: usize,
    row: usize,
    style: u8,
    color: u32,
    blink_on: bool,
}

#[derive(Clone)]
struct FontFamilyOption {
    label: String,
    family_name: Option<&'static str>,
}

const CURSOR_STYLE_BLOCK: u8 = 0;

impl Renderer {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        multisample: MultisampleState,
        width: u32,
        height: u32,
        font_size: f32,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let (available_font_families, font_family_index) =
            discover_font_family_options(&font_system);
        let font_family_name = available_font_families[font_family_index].family_name;
        let swash_cache = SwashCache::new();
        let line_height_factor = LINE_HEIGHT_FACTOR;
        let ligatures_enabled = true;
        let line_height = font_size * line_height_factor;
        let cell_width = measure_monospace_cell_width(
            &mut font_system,
            line_height,
            font_size,
            font_family_name,
        );

        let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
        buffer.set_size(&mut font_system, Some(width as f32), Some(height as f32));
        buffer.set_monospace_width(&mut font_system, Some(cell_width));
        buffer.shape_until_scroll(&mut font_system, true);

        let mut settings_button_buffer =
            Buffer::new(&mut font_system, Metrics::new(UI_FONT_SIZE, UI_LINE_HEIGHT));
        settings_button_buffer.set_size(&mut font_system, Some(120.0), Some(TAB_HEIGHT as f32));
        let settings_title_buffer = Buffer::new(
            &mut font_system,
            Metrics::new(UI_TITLE_FONT_SIZE, UI_TITLE_LINE_HEIGHT),
        );

        let cache = Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer = TextRenderer::new(
            &mut atlas,
            device,
            multisample,
            None::<wgpu::DepthStencilState>,
        );
        let viewport = glyphon::Viewport::new(device, &cache);

        let solid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Solid Rect Shader"),
            source: wgpu::ShaderSource::Wgsl(SOLID_SHADER.into()),
        });
        let solid_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Solid Rect Pipeline Layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
        let solid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Solid Rect Pipeline"),
            layout: Some(&solid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &solid_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[SolidVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &solid_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample,
            multiview_mask: None,
            cache: None,
        });
        let solid_vertex_capacity = INITIAL_SOLID_VERTEX_CAPACITY;
        let solid_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Solid Rect Vertex Buffer"),
            size: (solid_vertex_capacity * std::mem::size_of::<SolidVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            font_system,
            font_family_name,
            available_font_families,
            font_family_index,
            initial_font_family_index: font_family_index,
            ligatures_enabled,
            initial_ligatures_enabled: ligatures_enabled,
            swash_cache,
            buffer,
            tab_buffer: Vec::new(),
            tabs_cache: Vec::new(),
            tabs_need_shape: false,
            settings_button_buffer,
            settings_title_buffer,
            settings_sidebar_buffers: Vec::new(),
            settings_item_buffers: Vec::new(),
            settings_detail_buffers: Vec::new(),
            settings_value_buffers: Vec::new(),
            settings_cache: None,
            settings_need_shape: false,
            atlas,
            text_renderer,
            viewport,
            text_format: format,
            text_multisample: multisample,
            width,
            height,
            line_height,
            cell_width,
            spans_cache: Vec::with_capacity(256),
            solid_vertices: Vec::with_capacity(INITIAL_SOLID_VERTEX_CAPACITY),
            background_vertex_count: 0,
            solid_vertex_capacity,
            solid_vertex_buffer,
            solid_pipeline,
            font_size,
            initial_font_size: font_size,
            line_height_factor,
            initial_line_height_factor: line_height_factor,
            needs_shape: false,
            last_grid: Vec::new(),
            last_cursor: None,
            full_rebuild: true,
        }
    }

    fn recreate_text_renderer(&mut self, device: &Device, queue: &Queue) {
        let cache = Cache::new(device);
        self.atlas = TextAtlas::new(device, queue, &cache, self.text_format);
        self.text_renderer = TextRenderer::new(
            &mut self.atlas,
            device,
            self.text_multisample,
            None::<wgpu::DepthStencilState>,
        );
        self.viewport = glyphon::Viewport::new(device, &cache);
    }

    pub fn reset_font_size(&mut self) {
        let size = self.initial_font_size;
        self.set_font_size(size);
    }

    pub fn set_font_size(&mut self, font_size: f32) {
        const MIN_FONT_SIZE: f32 = 6.0;
        const MAX_FONT_SIZE: f32 = 72.0;

        let font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if (font_size - self.font_size).abs() < 0.01 {
            return;
        }

        self.font_size = font_size;
        self.refresh_typography();
    }

    pub fn current_font_family_label(&self) -> &str {
        &self.available_font_families[self.font_family_index].label
    }

    pub fn cycle_font_family(&mut self) {
        if self.available_font_families.len() <= 1 {
            return;
        }
        self.font_family_index = (self.font_family_index + 1) % self.available_font_families.len();
        self.font_family_name = self.available_font_families[self.font_family_index].family_name;
        self.refresh_typography();
    }

    pub fn ligatures_enabled(&self) -> bool {
        self.ligatures_enabled
    }

    pub fn toggle_ligatures(&mut self) {
        self.ligatures_enabled = !self.ligatures_enabled;
        self.refresh_typography();
    }

    pub fn line_height_factor(&self) -> f32 {
        self.line_height_factor
    }

    pub fn cycle_line_height(&mut self) {
        const PRESETS: [f32; 5] = [1.0, 1.15, 1.2, 1.35, 1.5];

        let current_index = PRESETS
            .iter()
            .position(|v| (*v - self.line_height_factor).abs() < 0.01)
            .unwrap_or(0);
        self.line_height_factor = PRESETS[(current_index + 1) % PRESETS.len()];
        self.refresh_typography();
    }

    pub fn reset_appearance(&mut self) {
        self.font_size = self.initial_font_size;
        self.font_family_index = self.initial_font_family_index;
        self.font_family_name = self.available_font_families[self.font_family_index].family_name;
        self.ligatures_enabled = self.initial_ligatures_enabled;
        self.line_height_factor = self.initial_line_height_factor;
        self.refresh_typography();
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.line_height)
    }

    pub fn visible_row_capacity(&self) -> usize {
        ((self.height as f32 - TAB_HEIGHT as f32 - 2.0 * TERMINAL_PADDING_Y).max(0.0)
            / self.line_height)
            .floor()
            .max(1.0) as usize
    }

    pub fn shaping_mode(&self) -> Shaping {
        if self.ligatures_enabled {
            Shaping::Advanced
        } else {
            Shaping::Basic
        }
    }

    pub(super) fn content_left(&self) -> f32 {
        TERMINAL_PADDING_X
    }

    pub(super) fn content_top(&self) -> f32 {
        TAB_HEIGHT as f32 + TERMINAL_PADDING_Y
    }

    pub(super) fn content_width(&self) -> f32 {
        (self.width as f32 - 2.0 * TERMINAL_PADDING_X).max(0.0)
    }

    pub(super) fn content_height(&self) -> f32 {
        (self.height as f32 - TAB_HEIGHT as f32 - 2.0 * TERMINAL_PADDING_Y).max(0.0)
    }

    fn refresh_typography(&mut self) {
        self.line_height = self.font_size * self.line_height_factor;
        self.cell_width = measure_monospace_cell_width(
            &mut self.font_system,
            self.line_height,
            self.font_size,
            self.font_family_name,
        );

        self.buffer = self.new_text_buffer(self.width as f32, self.height as f32);
        self.tab_buffer = Vec::new();
        self.settings_button_buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(UI_FONT_SIZE, UI_LINE_HEIGHT),
        );
        self.settings_title_buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(UI_TITLE_FONT_SIZE, UI_TITLE_LINE_HEIGHT),
        );
        self.settings_sidebar_buffers.clear();
        self.settings_item_buffers.clear();
        self.settings_detail_buffers.clear();
        self.settings_value_buffers.clear();
        self.tabs_need_shape = true;
        self.needs_shape = true;
        self.settings_need_shape = true;
        self.full_rebuild = true;
        self.last_grid.clear();
    }

    fn new_text_buffer(&mut self, width: f32, height: f32) -> Buffer {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(self.font_size, self.line_height),
        );
        buffer.set_size(&mut self.font_system, Some(width), Some(height));
        buffer.set_monospace_width(&mut self.font_system, Some(self.cell_width));
        buffer
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.buffer.set_size(
            &mut self.font_system,
            Some(width as f32),
            Some(height as f32),
        );
        // Row count may have changed — force full rebuild next frame
        self.full_rebuild = true;
        self.last_grid.clear();
    }

    pub fn set_cells(
        &mut self,
        rows: &[&Vec<Cell>],
        cursor: Option<CursorRenderInfo>,
        tabs: Option<Vec<TabRenderInfo>>,
        settings: Option<SettingsPanelRenderInfo>,
        scroll_indicator: Option<ScrollIndicatorRenderInfo>,
        content_changed_hint: bool,
    ) {
        let cursor_block_cell = cursor.and_then(|c| match c.style {
            CursorRenderStyle::Block if c.blink_on => Some((c.col, c.row, c.color)),
            _ => None,
        });
        let shaping_mode = self.shaping_mode();

        let row_count = rows.len();

        // Cursor movement dirties both the old and new cursor rows.
        let new_cursor_state = cursor.map(cursor_cache_key);
        let cursor_changed = new_cursor_state != self.last_cursor;

        // Grow / shrink cache to match the current row count.
        if self.last_grid.len() != row_count {
            self.full_rebuild = true;
            self.last_grid.resize(row_count, Vec::new());
        }

        let dirty_info = renderer_dirty::compute_dirty_info(
            self,
            rows,
            cursor_changed,
            new_cursor_state,
            content_changed_hint,
        );

        self.tabs_cache = tabs.clone().unwrap_or_default();
        self.settings_cache = settings.clone();
        let ui_shaping = Shaping::Basic;

        if self.tabs_cache.len() != self.tab_buffer.len() {
            self.tab_buffer.resize_with(self.tabs_cache.len(), || {
                Buffer::new(
                    &mut self.font_system,
                    Metrics::new(self.font_size, self.line_height),
                )
            });
        }

        for i in 0..self.tabs_cache.len() {
            let tab = &self.tabs_cache[i];
            let color = if tab.active {
                Color::rgb(229, 229, 229)
            } else if tab.is_hovered {
                Color::rgb(255, 255, 255)
            } else {
                Color::rgb(160, 160, 160)
            };
            self.tab_buffer[i].set_size(
                &mut self.font_system,
                Some(tab.width as f32),
                Some(tab.height as f32),
            );
            let attrs = Attrs::new()
                .family(font_family(self.font_family_name))
                .color(color);
            self.tab_buffer[i].set_text(
                &mut self.font_system,
                &tab.title,
                &attrs,
                shaping_mode,
                Some(Align::Center),
            );
        }
        self.tabs_need_shape = true;

        if let Some(settings_info) = &self.settings_cache {
            self.settings_button_buffer.set_size(
                &mut self.font_system,
                Some(settings_info.button_rect.width as f32),
                Some(settings_info.button_rect.height as f32),
            );
            let button_attrs = Attrs::new()
                .family(font_family(self.font_family_name))
                .color(Color::rgb(232, 232, 232));
            self.settings_button_buffer.set_text(
                &mut self.font_system,
                "",
                &button_attrs,
                ui_shaping,
                Some(Align::Center),
            );

            if settings_info.is_open {
                self.settings_title_buffer.set_size(
                    &mut self.font_system,
                    Some(settings_info.content_rect.width as f32),
                    Some(36.0),
                );
                let title_attrs = Attrs::new()
                    .family(font_family(self.font_family_name))
                    .color(Color::rgb(245, 247, 250))
                    .weight(Weight::BOLD);
                self.settings_title_buffer.set_text(
                    &mut self.font_system,
                    "Preferences",
                    &title_attrs,
                    ui_shaping,
                    None,
                );

                const SIDEBAR_LABELS: [&str; 3] = ["Appearance", "Window", "Keys"];
                if self.settings_sidebar_buffers.len() != SIDEBAR_LABELS.len() {
                    self.settings_sidebar_buffers
                        .resize_with(SIDEBAR_LABELS.len(), || {
                            Buffer::new(
                                &mut self.font_system,
                                Metrics::new(UI_FONT_SIZE, UI_LINE_HEIGHT),
                            )
                        });
                }
                for (i, label) in SIDEBAR_LABELS.iter().enumerate() {
                    self.settings_sidebar_buffers[i].set_size(
                        &mut self.font_system,
                        Some(settings_info.sidebar_rect.width as f32 - 24.0),
                        Some(32.0),
                    );
                    let color = if i == 0 {
                        Color::rgb(245, 247, 250)
                    } else {
                        Color::rgb(139, 148, 160)
                    };
                    let attrs = Attrs::new()
                        .family(font_family(self.font_family_name))
                        .color(color);
                    self.settings_sidebar_buffers[i].set_text(
                        &mut self.font_system,
                        label,
                        &attrs,
                        ui_shaping,
                        None,
                    );
                }

                if self.settings_item_buffers.len() != settings_info.items.len() {
                    self.settings_item_buffers
                        .resize_with(settings_info.items.len(), || {
                            Buffer::new(
                                &mut self.font_system,
                                Metrics::new(UI_FONT_SIZE, UI_LINE_HEIGHT),
                            )
                        });
                }
                if self.settings_value_buffers.len() != settings_info.items.len() {
                    self.settings_value_buffers
                        .resize_with(settings_info.items.len(), || {
                            Buffer::new(
                                &mut self.font_system,
                                Metrics::new(UI_VALUE_FONT_SIZE, UI_VALUE_LINE_HEIGHT),
                            )
                        });
                }
                if self.settings_detail_buffers.len() != settings_info.items.len() {
                    self.settings_detail_buffers
                        .resize_with(settings_info.items.len(), || {
                            Buffer::new(
                                &mut self.font_system,
                                Metrics::new(UI_DETAIL_FONT_SIZE, UI_DETAIL_LINE_HEIGHT),
                            )
                        });
                }

                for (i, item) in settings_info.items.iter().enumerate() {
                    self.settings_item_buffers[i].set_size(
                        &mut self.font_system,
                        Some(item.rect.width.saturating_sub(190) as f32),
                        Some(UI_LINE_HEIGHT + 4.0),
                    );
                    self.settings_detail_buffers[i].set_size(
                        &mut self.font_system,
                        Some(item.rect.width.saturating_sub(190) as f32),
                        Some(UI_DETAIL_LINE_HEIGHT + 3.0),
                    );
                    self.settings_value_buffers[i].set_size(
                        &mut self.font_system,
                        Some(item.primary_rect.width as f32),
                        Some(item.primary_rect.height as f32),
                    );
                    let color = if item.is_hovered {
                        Color::rgb(255, 255, 255)
                    } else {
                        Color::rgb(222, 222, 222)
                    };
                    let attrs = Attrs::new()
                        .family(font_family(self.font_family_name))
                        .color(color)
                        .weight(Weight::BOLD);
                    self.settings_item_buffers[i].set_text(
                        &mut self.font_system,
                        &item.title,
                        &attrs,
                        ui_shaping,
                        None,
                    );
                    let detail_attrs = Attrs::new()
                        .family(font_family(self.font_family_name))
                        .color(Color::rgb(145, 154, 166));
                    self.settings_detail_buffers[i].set_text(
                        &mut self.font_system,
                        &item.detail,
                        &detail_attrs,
                        ui_shaping,
                        None,
                    );
                    let value_attrs = Attrs::new()
                        .family(font_family(self.font_family_name))
                        .color(Color::rgb(232, 238, 245));
                    self.settings_value_buffers[i].set_text(
                        &mut self.font_system,
                        &item.value,
                        &value_attrs,
                        ui_shaping,
                        Some(Align::Center),
                    );
                }
            } else {
                self.settings_sidebar_buffers.clear();
                self.settings_item_buffers.clear();
                self.settings_detail_buffers.clear();
                self.settings_value_buffers.clear();
            }
            self.settings_need_shape = true;
        }

        if !dirty_info.any_dirty {
            // Keep overlay layers (cursor/tab/scroll indicator) responsive even
            // when text/background content did not change.
            if self.solid_vertices.len() > self.background_vertex_count {
                self.solid_vertices.truncate(self.background_vertex_count);
            }

            renderer_cursor::render_cursor_overlay(self, cursor);
            renderer_tab::render_tab_overlay(self, tabs, settings);
            renderer_scroll_indicator::render_scroll_indicator_overlay(self, scroll_indicator);

            self.last_cursor = new_cursor_state;
            self.full_rebuild = false;
            return;
        }

        // ── Background geometry ──────────────────────────────────────────────
        let rebuild_background = dirty_info.any_content_dirty || self.background_vertex_count == 0;
        if rebuild_background {
            renderer_background::rebuild_background_geometry(self, rows);
        } else if self.solid_vertices.len() > self.background_vertex_count {
            // Drop old cursor overlay vertices, keep the background vertices.
            self.solid_vertices.truncate(self.background_vertex_count);
        }

        // ── Text spans ──────────────────────────────────────────────────────
        if dirty_info.needs_text_rebuild {
            renderer_text::rebuild_text_spans(self, rows, cursor_block_cell);
        }

        // ── Cursor overlay geometry ───────────────────────────────────────────
        renderer_cursor::render_cursor_overlay(self, cursor);

        renderer_tab::render_tab_overlay(self, tabs, settings);
        renderer_scroll_indicator::render_scroll_indicator_overlay(self, scroll_indicator);

        // ── Update the cache snapshot ─────────────────────────────────────────
        renderer_cache::update_last_grid_snapshot(self, rows, &dirty_info.content_dirty);

        self.last_cursor = new_cursor_state;
        self.full_rebuild = false;
    }

    pub fn prepare(&mut self, device: &Device, queue: &Queue, fg_color: Color) {
        if self.needs_shape {
            self.buffer.shape_until_scroll(&mut self.font_system, true);
            self.needs_shape = false;
        }
        if self.tabs_need_shape {
            for b in &mut self.tab_buffer {
                b.shape_until_scroll(&mut self.font_system, true);
            }
            self.tabs_need_shape = false;
        }
        if self.settings_need_shape {
            self.settings_button_buffer
                .shape_until_scroll(&mut self.font_system, true);
            self.settings_title_buffer
                .shape_until_scroll(&mut self.font_system, true);
            for b in &mut self.settings_sidebar_buffers {
                b.shape_until_scroll(&mut self.font_system, true);
            }
            for b in &mut self.settings_item_buffers {
                b.shape_until_scroll(&mut self.font_system, true);
            }
            for b in &mut self.settings_detail_buffers {
                b.shape_until_scroll(&mut self.font_system, true);
            }
            for b in &mut self.settings_value_buffers {
                b.shape_until_scroll(&mut self.font_system, true);
            }
            self.settings_need_shape = false;
        }

        self.viewport.update(
            queue,
            glyphon::Resolution {
                width: self.width,
                height: self.height,
            },
        );

        let prepare_once = |this: &mut Self| {
            let content_top = this.content_top();
            let content_left = this.content_left();
            let content_right = content_left + this.content_width();
            let content_bottom = content_top + this.content_height();
            let mut areas = Vec::with_capacity(1 + this.tab_buffer.len());

            let settings_open = this
                .settings_cache
                .as_ref()
                .map(|settings| settings.is_open)
                .unwrap_or(false);
            if !settings_open {
                areas.push(TextArea {
                    buffer: &this.buffer,
                    left: content_left,
                    top: content_top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: content_left as i32,
                        top: content_top as i32,
                        right: content_right as i32,
                        bottom: content_bottom as i32,
                    },
                    default_color: fg_color,
                    custom_glyphs: &[],
                });
            } else if let Some(settings) = &this.settings_cache {
                let panel_left = settings.panel_rect.x as f32;
                let panel_top = settings.panel_rect.y as f32;
                let panel_right = (settings.panel_rect.x + settings.panel_rect.width) as f32;
                let panel_bottom = (settings.panel_rect.y + settings.panel_rect.height) as f32;

                let mut push_terminal_clip = |left: f32, top: f32, right: f32, bottom: f32| {
                    let left = left.max(content_left).min(content_right);
                    let right = right.max(content_left).min(content_right);
                    let top = top.max(content_top).min(content_bottom);
                    let bottom = bottom.max(content_top).min(content_bottom);
                    if right <= left || bottom <= top {
                        return;
                    }
                    areas.push(TextArea {
                        buffer: &this.buffer,
                        left: content_left,
                        top: content_top,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: left as i32,
                            top: top as i32,
                            right: right as i32,
                            bottom: bottom as i32,
                        },
                        default_color: fg_color,
                        custom_glyphs: &[],
                    });
                };

                push_terminal_clip(content_left, content_top, content_right, panel_top);
                push_terminal_clip(content_left, panel_bottom, content_right, content_bottom);
                push_terminal_clip(content_left, panel_top, panel_left, panel_bottom);
                push_terminal_clip(panel_right, panel_top, content_right, panel_bottom);
            }
            for i in 0..this.tabs_cache.len() {
                let tab = this.tabs_cache[i].clone();
                let text_left = tab.x as f32;
                let text_top = tab.y as f32 + (tab.height as f32 - this.line_height) * 0.5;
                let tab_area = TextArea {
                    buffer: &this.tab_buffer[i],
                    left: text_left,
                    top: text_top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: tab.x as i32,
                        top: tab.y as i32,
                        right: (tab.x + tab.width) as i32,
                        bottom: (tab.y + tab.height) as i32,
                    },
                    default_color: fg_color,
                    custom_glyphs: &[],
                };
                areas.push(tab_area);
            }

            if let Some(settings) = &this.settings_cache {
                let button_area = TextArea {
                    buffer: &this.settings_button_buffer,
                    left: settings.button_rect.x as f32,
                    top: settings.button_rect.y as f32
                        + (settings.button_rect.height as f32 - UI_LINE_HEIGHT) * 0.5,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: settings.button_rect.x as i32,
                        top: settings.button_rect.y as i32,
                        right: (settings.button_rect.x + settings.button_rect.width) as i32,
                        bottom: (settings.button_rect.y + settings.button_rect.height) as i32,
                    },
                    default_color: fg_color,
                    custom_glyphs: &[],
                };
                areas.push(button_area);

                if settings.is_open {
                    let title_area = TextArea {
                        buffer: &this.settings_title_buffer,
                        left: settings.content_rect.x as f32,
                        top: settings.content_rect.y as f32 + 18.0,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: settings.content_rect.x as i32,
                            top: settings.content_rect.y as i32,
                            right: (settings.content_rect.x + settings.content_rect.width) as i32,
                            bottom: (settings.content_rect.y + 56) as i32,
                        },
                        default_color: fg_color,
                        custom_glyphs: &[],
                    };
                    areas.push(title_area);

                    for (i, buffer) in this.settings_sidebar_buffers.iter().enumerate() {
                        let top = settings.sidebar_rect.y as f32 + 72.0 + i as f32 * 38.0;
                        let sidebar_area = TextArea {
                            buffer,
                            left: settings.sidebar_rect.x as f32 + 18.0,
                            top,
                            scale: 1.0,
                            bounds: TextBounds {
                                left: (settings.sidebar_rect.x + 18) as i32,
                                top: top as i32,
                                right: (settings.sidebar_rect.x + settings.sidebar_rect.width - 10)
                                    as i32,
                                bottom: (top + 34.0) as i32,
                            },
                            default_color: fg_color,
                            custom_glyphs: &[],
                        };
                        areas.push(sidebar_area);
                    }

                    for (i, item) in settings.items.iter().enumerate() {
                        if i >= this.settings_item_buffers.len() {
                            break;
                        }
                        let item_area = TextArea {
                            buffer: &this.settings_item_buffers[i],
                            left: item.rect.x as f32 + 18.0,
                            top: item.rect.y as f32 + 13.0,
                            scale: 1.0,
                            bounds: TextBounds {
                                left: (item.rect.x + 18) as i32,
                                top: item.rect.y as i32,
                                right: (item.rect.x + item.rect.width) as i32,
                                bottom: (item.rect.y + 38) as i32,
                            },
                            default_color: fg_color,
                            custom_glyphs: &[],
                        };
                        areas.push(item_area);

                        if i < this.settings_detail_buffers.len() {
                            let detail_area = TextArea {
                                buffer: &this.settings_detail_buffers[i],
                                left: item.rect.x as f32 + 18.0,
                                top: item.rect.y as f32 + 39.0,
                                scale: 1.0,
                                bounds: TextBounds {
                                    left: (item.rect.x + 18) as i32,
                                    top: (item.rect.y + 36) as i32,
                                    right: (item.rect.x + item.rect.width.saturating_sub(178))
                                        as i32,
                                    bottom: (item.rect.y + item.rect.height) as i32,
                                },
                                default_color: fg_color,
                                custom_glyphs: &[],
                            };
                            areas.push(detail_area);
                        }

                        if i < this.settings_value_buffers.len() {
                            let value_area = TextArea {
                                buffer: &this.settings_value_buffers[i],
                                left: item.primary_rect.x as f32,
                                top: item.primary_rect.y as f32
                                    + (item.primary_rect.height as f32 - UI_VALUE_LINE_HEIGHT)
                                        * 0.5,
                                scale: 1.0,
                                bounds: TextBounds {
                                    left: item.primary_rect.x as i32,
                                    top: item.primary_rect.y as i32,
                                    right: (item.primary_rect.x + item.primary_rect.width) as i32,
                                    bottom: (item.primary_rect.y + item.primary_rect.height) as i32,
                                },
                                default_color: fg_color,
                                custom_glyphs: &[],
                            };
                            areas.push(value_area);
                        }
                    }
                }
            }

            this.text_renderer.prepare(
                device,
                queue,
                &mut this.font_system,
                &mut this.atlas,
                &this.viewport,
                areas,
                &mut this.swash_cache,
            )
        };

        if let Err(err) = prepare_once(self) {
            self.recreate_text_renderer(device, queue);
            if let Err(err2) = prepare_once(self) {
                eprintln!("glyphon prepare failed after atlas reset: {err:?} -> {err2:?}");
            }
        }
    }

    pub fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        fg_color: Color,
    ) {
        self.prepare(device, queue, fg_color);

        {
            let mut bg_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Background Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            if !self.solid_vertices.is_empty() {
                self.ensure_solid_vertex_buffer(device, self.solid_vertices.len());
                queue.write_buffer(
                    &self.solid_vertex_buffer,
                    0,
                    bytemuck::cast_slice(&self.solid_vertices),
                );
                bg_pass.set_pipeline(&self.solid_pipeline);
                bg_pass.set_vertex_buffer(0, self.solid_vertex_buffer.slice(..));
                bg_pass.draw(0..self.solid_vertices.len() as u32, 0..1);
            }
        }

        let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });

        if let Err(err) = self
            .text_renderer
            .render(&self.atlas, &self.viewport, &mut text_pass)
        {
            eprintln!("glyphon render failed: {err:?}");
        }
    }

    fn ensure_solid_vertex_buffer(&mut self, device: &Device, required_vertices: usize) {
        if required_vertices <= self.solid_vertex_capacity {
            return;
        }
        self.solid_vertex_capacity = required_vertices
            .next_power_of_two()
            .max(INITIAL_SOLID_VERTEX_CAPACITY);
        self.solid_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Solid Rect Vertex Buffer"),
            size: (self.solid_vertex_capacity * std::mem::size_of::<SolidVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }

    fn push_rect_cells(
        &mut self,
        col: usize,
        row: usize,
        col_span: usize,
        row_span: usize,
        color: Color,
        alpha_scale: f32,
    ) {
        if col_span == 0 || row_span == 0 {
            return;
        }
        self.push_rect_pixels(
            col as f32 * self.cell_width + self.content_left(),
            row as f32 * self.line_height + self.content_top(),
            col_span as f32 * self.cell_width,
            row_span as f32 * self.line_height,
            color,
            alpha_scale,
        );
    }

    fn push_rect_pixels(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color, alpha_scale: f32) {
        if self.width == 0 || self.height == 0 || w <= 0.0 || h <= 0.0 {
            return;
        }

        let x0 = (x / self.width as f32) * 2.0 - 1.0;
        let x1 = ((x + w) / self.width as f32) * 2.0 - 1.0;
        let y0 = 1.0 - (y / self.height as f32) * 2.0;
        let y1 = 1.0 - ((y + h) / self.height as f32) * 2.0;

        let mut rgba = color_to_rgba_f32(color);
        rgba[3] *= alpha_scale.clamp(0.0, 1.0);

        self.solid_vertices.extend_from_slice(&[
            SolidVertex {
                position: [x0, y0],
                color: rgba,
            },
            SolidVertex {
                position: [x1, y0],
                color: rgba,
            },
            SolidVertex {
                position: [x0, y1],
                color: rgba,
            },
            SolidVertex {
                position: [x1, y0],
                color: rgba,
            },
            SolidVertex {
                position: [x1, y1],
                color: rgba,
            },
            SolidVertex {
                position: [x0, y1],
                color: rgba,
            },
        ]);
    }
}

fn effective_colors(cell: &Cell) -> (Color, Color) {
    if cell.is_selected {
        return (Color::rgb(12, 16, 24), Color::rgb(176, 212, 255));
    }
    if cell.style & style::REVERSE != 0 {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    }
}

fn build_attrs(cell: &Cell, fg: Color, font_family_name: Option<&'static str>) -> Attrs<'static> {
    let mut attrs = Attrs::new().family(font_family(font_family_name)).color(fg);

    if cell.style & style::BOLD != 0 {
        attrs = attrs.weight(Weight::BOLD);
    }
    if cell.style & style::ITALIC != 0 {
        attrs = attrs.style(Style::Italic);
    }
    if cell.style & style::DIM != 0 {
        let r = fg.r() / 2;
        let g = fg.g() / 2;
        let b = fg.b() / 2;
        let a = fg.a();
        attrs = attrs.color(Color::rgba(r, g, b, a));
    }
    if cell.style & style::HIDDEN != 0 {
        attrs = attrs.color(cell.bg);
    }

    attrs
}

fn detect_primary_nerd_mono_font_family(font_system: &FontSystem) -> Option<&'static str> {
    let mut preferred_text_nerd_mono: Option<String> = None;
    let mut any_nerd_mono: Option<String> = None;

    for face in font_system.db().faces() {
        for (name, _) in &face.families {
            let lower = name.to_ascii_lowercase();
            let is_nerd = lower.contains("nerd font") || lower.ends_with(" nf");
            let is_mono = lower.contains("nerd font mono") || lower.ends_with(" nf mono");
            if !is_nerd || !is_mono {
                continue;
            }

            let is_symbols_only =
                lower.contains("symbols nerd font mono") || lower.contains("symbols nf mono");
            if preferred_text_nerd_mono.is_none() && !is_symbols_only {
                preferred_text_nerd_mono = Some(name.clone());
            }
            if any_nerd_mono.is_none() {
                any_nerd_mono = Some(name.clone());
            }
        }
    }

    preferred_text_nerd_mono
        .or(any_nerd_mono)
        .map(|name| Box::leak(name.into_boxed_str()) as &'static str)
}

fn discover_font_family_options(font_system: &FontSystem) -> (Vec<FontFamilyOption>, usize) {
    let mut discovered = BTreeMap::new();
    for face in font_system.db().faces() {
        for (name, _) in &face.families {
            let lower = name.to_ascii_lowercase();
            if lower.contains("symbols") {
                continue;
            }
            if lower.contains("mono")
                || lower.contains("code")
                || lower.contains("terminal")
                || lower.contains("console")
            {
                discovered.entry(lower).or_insert_with(|| name.clone());
            }
        }
    }

    let mut options = vec![FontFamilyOption {
        label: "System Mono".to_string(),
        family_name: None,
    }];

    for name in discovered.into_values().take(8) {
        options.push(FontFamilyOption {
            label: name.clone(),
            family_name: Some(Box::leak(name.into_boxed_str()) as &'static str),
        });
    }

    let preferred = detect_primary_nerd_mono_font_family(font_system);
    if let Some(preferred_name) = preferred {
        let already_present = options
            .iter()
            .any(|opt| opt.family_name == Some(preferred_name));
        if !already_present {
            options.push(FontFamilyOption {
                label: preferred_name.to_string(),
                family_name: Some(preferred_name),
            });
        }
    }
    let selected_index = preferred
        .and_then(|preferred_name| {
            options
                .iter()
                .position(|opt| opt.family_name == Some(preferred_name))
        })
        .unwrap_or(0);

    (options, selected_index)
}

fn font_family(font_family_name: Option<&'static str>) -> Family<'static> {
    if let Some(name) = font_family_name {
        Family::Name(name)
    } else {
        Family::Monospace
    }
}

fn attrs_equal(a: &Attrs, b: &Attrs) -> bool {
    let colors_match = match (a.color_opt, b.color_opt) {
        (Some(ca), Some(cb)) => ca.0 == cb.0,
        (None, None) => true,
        _ => false,
    };
    colors_match && a.weight == b.weight && a.style == b.style && a.family == b.family
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn color_to_rgba_f32(color: Color) -> [f32; 4] {
    let r = srgb_to_linear(color.r() as f32 / 255.0);
    let g = srgb_to_linear(color.g() as f32 / 255.0);
    let b = srgb_to_linear(color.b() as f32 / 255.0);
    let a = color.a() as f32 / 255.0;
    [r, g, b, a]
}

fn contrast_text_color(background: Color) -> Color {
    let r = background.r() as f32;
    let g = background.g() as f32;
    let b = background.b() as f32;
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if luma < 128.0 {
        Color::rgb(255, 255, 255)
    } else {
        Color::rgb(0, 0, 0)
    }
}

fn cursor_cache_key(cursor: CursorRenderInfo) -> CursorCacheKey {
    let style = match cursor.style {
        CursorRenderStyle::Block => 0,
        CursorRenderStyle::Underline => 1,
        CursorRenderStyle::Bar => 2,
        CursorRenderStyle::Unfocused => 3,
    };

    CursorCacheKey {
        col: cursor.col,
        row: cursor.row,
        style,
        color: cursor.color.0,
        blink_on: cursor.blink_on,
    }
}

fn measure_monospace_cell_width(
    font_system: &mut FontSystem,
    line_height: f32,
    font_size: f32,
    font_family_name: Option<&'static str>,
) -> f32 {
    let fallback = font_size * CELL_WIDTH_FACTOR;
    let sample = "M".repeat(CELL_WIDTH_SAMPLE_COUNT);
    let attrs = Attrs::new().family(font_family(font_family_name));

    let mut probe = Buffer::new(font_system, Metrics::new(font_size, line_height));
    probe.set_size(font_system, None, None);
    probe.set_text(font_system, &sample, &attrs, Shaping::Basic, None);

    for run in probe.layout_runs() {
        if !run.glyphs.is_empty() && run.line_w > 0.0 {
            let measured = run.line_w / CELL_WIDTH_SAMPLE_COUNT as f32;
            if measured.is_finite() && measured > 0.0 {
                return measured;
            }
        }
    }

    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_to_linear_endpoints() {
        assert!((srgb_to_linear(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn srgb_to_linear_is_monotonic_for_simple_samples() {
        let a = srgb_to_linear(0.1);
        let b = srgb_to_linear(0.2);
        let c = srgb_to_linear(0.7);
        assert!(a < b && b < c);
    }

    #[test]
    fn color_to_rgba_f32_preserves_alpha_and_converts_channels() {
        let c = Color::rgba(255, 0, 0, 128);
        let rgba = color_to_rgba_f32(c);
        assert!((rgba[0] - 1.0).abs() < 1e-6);
        assert!(rgba[1] >= 0.0 && rgba[1] < 1e-6);
        assert!(rgba[2] >= 0.0 && rgba[2] < 1e-6);
        assert!((rgba[3] - (128.0 / 255.0)).abs() < 1e-6);
    }

    #[test]
    fn contrast_text_color_picks_white_for_dark_background() {
        assert_eq!(
            contrast_text_color(Color::rgb(0, 0, 0)),
            Color::rgb(255, 255, 255)
        );
        assert_eq!(
            contrast_text_color(Color::rgb(10, 10, 10)),
            Color::rgb(255, 255, 255)
        );
    }

    #[test]
    fn contrast_text_color_picks_black_for_bright_background() {
        assert_eq!(
            contrast_text_color(Color::rgb(255, 255, 255)),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(
            contrast_text_color(Color::rgb(240, 240, 240)),
            Color::rgb(0, 0, 0)
        );
    }

    #[test]
    fn cursor_cache_key_maps_styles_and_fields() {
        let c = CursorRenderInfo {
            col: 3,
            row: 4,
            style: CursorRenderStyle::Underline,
            color: Color::rgb(1, 2, 3),
            blink_on: true,
        };
        let key = cursor_cache_key(c);
        assert_eq!(key.col, 3);
        assert_eq!(key.row, 4);
        assert_eq!(key.style, 1);
        assert_eq!(key.color, Color::rgb(1, 2, 3).0);
        assert!(key.blink_on);
    }
}
