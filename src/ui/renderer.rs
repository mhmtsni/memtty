use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

use crate::terminal::{Cell, style};

const LINE_HEIGHT_FACTOR: f32 = 1.2;
const CELL_WIDTH_FACTOR: f32 = 0.55;
const INITIAL_SOLID_VERTEX_CAPACITY: usize = 2048;
const CELL_WIDTH_SAMPLE_COUNT: usize = 32;

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
}

#[derive(Clone, Copy)]
pub struct CursorRenderInfo {
    pub col: usize,
    pub row: usize,
    pub style: CursorRenderStyle,
    pub color: Color,
    pub blink_on: bool,
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
    fg: u32,
    bg: u32,
    style: u8,
}

impl CellKey {
    fn from_cell(cell: &Cell) -> Self {
        Self {
            c: cell.c,
            fg: cell.fg.0,
            bg: cell.bg.0,
            style: cell.style,
        }
    }
}

pub struct Renderer {
    font_system: FontSystem,
    font_family_name: Option<&'static str>,
    swash_cache: SwashCache,
    buffer: Buffer,
    pub atlas: TextAtlas,
    pub text_renderer: TextRenderer,
    viewport: Viewport,
    text_format: TextureFormat,
    text_multisample: MultisampleState,
    pub width: u32,
    pub height: u32,
    line_height: f32,
    cell_width: f32,
    spans_cache: Vec<(String, Attrs<'static>)>,
    solid_vertices: Vec<SolidVertex>,
    background_vertex_count: usize,
    solid_vertex_capacity: usize,
    solid_vertex_buffer: wgpu::Buffer,
    solid_pipeline: wgpu::RenderPipeline,
    pub font_size: f32,
    initial_font_size: f32,
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

const CURSOR_STYLE_BLOCK: u8 = 0;

impl Renderer {
    pub fn active_font_family_name(&self) -> Option<&'static str> {
        self.font_family_name
    }

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
        let font_family_name = detect_primary_nerd_mono_font_family(&font_system);
        let swash_cache = SwashCache::new();
        let line_height = font_size * LINE_HEIGHT_FACTOR;
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
            swash_cache,
            buffer,
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
        self.line_height = self.font_size * LINE_HEIGHT_FACTOR;
        self.cell_width = measure_monospace_cell_width(
            &mut self.font_system,
            self.line_height,
            self.font_size,
            self.font_family_name,
        );

        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(self.font_size, self.line_height),
        );
        buffer.set_size(
            &mut self.font_system,
            Some(self.width as f32),
            Some(self.height as f32),
        );
        buffer.set_monospace_width(&mut self.font_system, Some(self.cell_width));
        self.buffer = buffer;
        self.needs_shape = true;
        // Font size change means all cached geometry is wrong (pixel coords changed)
        self.full_rebuild = true;
        self.last_grid.clear();
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.line_height)
    }

    pub fn visible_row_capacity(&self) -> usize {
        (self.height as f32 / self.line_height).floor().max(1.0) as usize
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
        content_changed_hint: bool,
    ) {
        let cursor_block_cell = cursor.and_then(|c| match c.style {
            CursorRenderStyle::Block if c.blink_on => Some((c.col, c.row, c.color)),
            _ => None,
        });

        // ── Determine which rows are dirty ────────────────────────────────────
        let row_count = rows.len();

        // Cursor movement dirties both the old and new cursor row so the
        // block highlight is painted/erased correctly.
        let new_cursor_state = cursor.map(cursor_cache_key);
        let cursor_changed = new_cursor_state != self.last_cursor;

        // Grow / shrink the cache to match the current row count.
        if self.last_grid.len() != row_count {
            self.full_rebuild = true;
            self.last_grid.resize(row_count, Vec::new());
        }

        // Build per-row dirty flags:
        // - content_dirty: row text/background changed
        // - dirty: any change that can affect rendering (incl. cursor movement)
        let mut content_dirty = vec![self.full_rebuild; row_count];
        let mut dirty = vec![self.full_rebuild; row_count];

        if !self.full_rebuild && !content_changed_hint {
            for (row_i, row) in rows.iter().enumerate() {
                // Content changed?
                let cache = &self.last_grid[row_i];
                if cache.len() != row.len() {
                    content_dirty[row_i] = true;
                    dirty[row_i] = true;
                    continue;
                }
                for (cell, &key) in row.iter().zip(cache.iter()) {
                    if CellKey::from_cell(cell) != key {
                        content_dirty[row_i] = true;
                        dirty[row_i] = true;
                        break;
                    }
                }
            }

            // Cursor movement dirties the rows it touches
            if cursor_changed {
                if let Some(old) = self.last_cursor {
                    let old_row = old.row;
                    if old_row < row_count {
                        dirty[old_row] = true;
                    }
                }
                if let Some(new_cursor) = new_cursor_state {
                    let new_row = new_cursor.row;
                    if new_row < row_count {
                        dirty[new_row] = true;
                    }
                }
            }
        } else if !self.full_rebuild && content_changed_hint {
            content_dirty.fill(true);
            dirty.fill(true);
        }

        let any_dirty = dirty.iter().any(|&d| d);
        if !any_dirty {
            return;
        }

        let any_content_dirty = content_dirty.iter().any(|&d| d);
        let prev_block_cursor_visible = self
            .last_cursor
            .map(|c| c.style == CURSOR_STYLE_BLOCK && c.blink_on)
            .unwrap_or(false);
        let new_block_cursor_visible = new_cursor_state
            .map(|c| c.style == CURSOR_STYLE_BLOCK && c.blink_on)
            .unwrap_or(false);
        // Block cursor inverts glyph color under cursor, so both old/new block
        // states require text rebuild even if cell content did not change.
        let cursor_affects_text = prev_block_cursor_visible || new_block_cursor_visible;
        let needs_text_rebuild = any_content_dirty || (cursor_changed && cursor_affects_text);

        if any_content_dirty || self.background_vertex_count == 0 {
            self.solid_vertices.clear();
            for (row_i, row) in rows.iter().enumerate() {
                if row.is_empty() {
                    continue;
                }
                let mut run_start = 0usize;
                let mut run_bg = effective_colors(&row[0]).1;
                for col in 1..=row.len() {
                    let next_bg = if col < row.len() {
                        effective_colors(&row[col]).1
                    } else {
                        Color::rgb(0, 0, 0)
                    };
                    if col == row.len() || next_bg.0 != run_bg.0 {
                        // Skip default black background runs; clear pass already paints black.
                        if run_bg.0 != Color::rgb(0, 0, 0).0 {
                            self.push_rect_cells(run_start, row_i, col - run_start, 1, run_bg, 1.0);
                        }
                        run_start = col;
                        run_bg = next_bg;
                    }
                }
            }
            self.background_vertex_count = self.solid_vertices.len();
        } else if self.solid_vertices.len() > self.background_vertex_count {
            self.solid_vertices.truncate(self.background_vertex_count);
        }

        if needs_text_rebuild {
            // ── Rebuild text spans ────────────────────────────────────────────
            for (s, _) in self.spans_cache.iter_mut() {
                s.clear();
            }
            let mut span_count = 0;

            for (row_i, row) in rows.iter().enumerate() {
                let last_non_space = row
                    .iter()
                    .rposition(|c| c.c != ' ')
                    .map(|i| i + 1)
                    .unwrap_or(0);

                for (col_i, cell) in row[..last_non_space].iter().enumerate() {
                    let (mut fg, _bg) = effective_colors(cell);

                    if let Some((cursor_col, cursor_row, cursor_color)) = cursor_block_cell {
                        if cursor_row == row_i && cursor_col == col_i {
                            fg = contrast_text_color(cursor_color);
                        }
                    }

                    let attrs = build_attrs(cell, fg, self.font_family_name);

                    if span_count > 0 && attrs_equal(&self.spans_cache[span_count - 1].1, &attrs) {
                        self.spans_cache[span_count - 1].0.push(cell.c);
                    } else {
                        if span_count < self.spans_cache.len() {
                            self.spans_cache[span_count].0.clear();
                            self.spans_cache[span_count].0.push(cell.c);
                            self.spans_cache[span_count].1 = attrs;
                        } else {
                            self.spans_cache.push((cell.c.to_string(), attrs));
                        }
                        span_count += 1;
                    }
                }

                if row_i + 1 < row_count {
                    if span_count > 0 {
                        self.spans_cache[span_count - 1].0.push('\n');
                    } else {
                        if span_count < self.spans_cache.len() {
                            self.spans_cache[span_count].0.clear();
                            self.spans_cache[span_count].0.push('\n');
                            self.spans_cache[span_count].1 = Attrs::new()
                                .family(font_family(self.font_family_name))
                                .color(Color::rgb(255, 255, 255));
                        } else {
                            self.spans_cache.push((
                                "\n".to_string(),
                                Attrs::new()
                                    .family(font_family(self.font_family_name))
                                    .color(Color::rgb(255, 255, 255)),
                            ));
                        }
                        span_count += 1;
                    }
                }
            }

            let active_spans = &self.spans_cache[..span_count];
            self.buffer.set_rich_text(
                &mut self.font_system,
                active_spans.iter().map(|(s, a)| (s.as_str(), a.clone())),
                &Attrs::new()
                    .family(font_family(self.font_family_name))
                    .color(Color::rgb(229, 229, 229)),
                Shaping::Basic,
                None::<glyphon::cosmic_text::Align>,
            );
            self.needs_shape = true;
        }

        // ── Cursor overlay geometry ───────────────────────────────────────────
        if let Some(cursor) = cursor {
            if !cursor.blink_on {
                // blink-off: still update cache/dirty but don't draw cursor
            } else {
                let cursor_alpha = 1.0;
                match cursor.style {
                    CursorRenderStyle::Block => self.push_rect_cells(
                        cursor.col,
                        cursor.row,
                        1,
                        1,
                        cursor.color,
                        cursor_alpha,
                    ),
                    CursorRenderStyle::Underline => {
                        let underline_height = (self.line_height * 0.12).max(2.0);
                        self.push_rect_pixels(
                            cursor.col as f32 * self.cell_width,
                            (cursor.row as f32 + 1.0) * self.line_height - underline_height,
                            self.cell_width,
                            underline_height,
                            cursor.color,
                            cursor_alpha,
                        );
                    }
                    CursorRenderStyle::Bar => {
                        let bar_width = (self.cell_width * 0.12).max(2.0);
                        self.push_rect_pixels(
                            cursor.col as f32 * self.cell_width,
                            cursor.row as f32 * self.line_height,
                            bar_width,
                            self.line_height,
                            cursor.color,
                            cursor_alpha,
                        );
                    }
                }
            }
        }

        // ── Update the cache snapshot ─────────────────────────────────────────
        for (row_i, row) in rows.iter().enumerate() {
            if content_dirty[row_i] {
                let cache_row = &mut self.last_grid[row_i];
                cache_row.resize(
                    row.len(),
                    CellKey {
                        c: ' ',
                        fg: 0,
                        bg: 0,
                        style: 0,
                    },
                );
                for (cell, key) in row.iter().zip(cache_row.iter_mut()) {
                    *key = CellKey::from_cell(cell);
                }
            }
        }
        self.last_cursor = new_cursor_state;
        self.full_rebuild = false;
    }

    pub fn prepare(&mut self, device: &Device, queue: &Queue, fg_color: Color) {
        if self.needs_shape {
            self.buffer.shape_until_scroll(&mut self.font_system, true);
            self.needs_shape = false;
        }

        self.viewport.update(
            queue,
            glyphon::Resolution {
                width: self.width,
                height: self.height,
            },
        );

        let prepare_once = |this: &mut Self| {
            this.text_renderer.prepare(
                device,
                queue,
                &mut this.font_system,
                &mut this.atlas,
                &this.viewport,
                vec![TextArea {
                    buffer: &this.buffer,
                    left: 0.0,
                    top: 0.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: this.width as i32,
                        bottom: this.height as i32,
                    },
                    default_color: fg_color,
                    custom_glyphs: &[],
                }],
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
            label: Some("Text Render Pass"),
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
            col as f32 * self.cell_width,
            row as f32 * self.line_height,
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
    if cell.style & style::REVERSE != 0 {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    }
}

fn build_attrs(cell: &Cell, fg: Color, font_family_name: Option<&'static str>) -> Attrs<'static> {
    let mut attrs = Attrs::new()
        .family(font_family(font_family_name))
        .color(fg);

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
