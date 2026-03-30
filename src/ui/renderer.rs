use glyphon::{
    Attrs, Buffer, Cache, Color, FontSystem, Metrics, Shaping, Style, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

use crate::terminal::{Cell, style};

const FONT_SIZE: f32 = 30.0;
const LINE_HEIGHT_FACTOR: f32 = 1.25;
const CELL_WIDTH_FACTOR: f32 = 0.55;

pub struct Renderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    buffer: Buffer,
    pub atlas: TextAtlas,
    pub text_renderer: TextRenderer,
    viewport: Viewport,
    // cache: Cache,
    width: u32,
    height: u32,
    line_height: f32,
    cell_width: f32,
    // Reused every frame — avoids per-frame Vec allocation
    spans_cache: Vec<(String, Attrs<'static>)>,
}

impl Renderer {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        multisample: MultisampleState,
        width: u32,
        height: u32,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let line_height = FONT_SIZE * LINE_HEIGHT_FACTOR;
        let cell_width = FONT_SIZE * CELL_WIDTH_FACTOR;

        let mut buffer = Buffer::new(&mut font_system, Metrics::new(FONT_SIZE, line_height));
        buffer.set_size(&mut font_system, Some(width as f32), Some(height as f32));
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

        Self {
            font_system,
            swash_cache,
            buffer,
            atlas,
            text_renderer,
            viewport,
            // cache,
            width,
            height,
            line_height,
            cell_width,
            spans_cache: Vec::with_capacity(256),
        }
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
        self.buffer.shape_until_scroll(&mut self.font_system, true);
    }

    pub fn set_cells(&mut self, rows: &[&Vec<Cell>]) {
        // Clear spans but keep allocation
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

            for (_col_i, cell) in row[..last_non_space].iter().enumerate() {
                let (fg, _bg) = effective_colors(cell);
                let attrs = build_attrs(cell, fg);

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

            // Newline between rows
            if row_i + 1 < rows.len() {
                if span_count > 0 {
                    self.spans_cache[span_count - 1].0.push('\n');
                } else {
                    if span_count < self.spans_cache.len() {
                        self.spans_cache[span_count].0.clear();
                        self.spans_cache[span_count].0.push('\n');
                        self.spans_cache[span_count].1 =
                            Attrs::new().color(Color::rgb(255, 255, 255));
                    } else {
                        self.spans_cache.push((
                            "\n".to_string(),
                            Attrs::new().color(Color::rgb(255, 255, 255)),
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
            &Attrs::new().color(Color::rgb(229, 229, 229)),
            Shaping::Advanced,
            None::<glyphon::cosmic_text::Align>,
        );

        self.buffer.shape_until_scroll(&mut self.font_system, true);
    }

    pub fn prepare(&mut self, device: &Device, queue: &Queue, fg_color: Color) {
        self.viewport.update(
            queue,
            glyphon::Resolution {
                width: self.width,
                height: self.height,
            },
        );

        self.text_renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                vec![TextArea {
                    buffer: &self.buffer,
                    left: 0.0,
                    top: 0.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.width as i32,
                        bottom: self.height as i32,
                    },
                    default_color: fg_color,
                    custom_glyphs: &[],
                }],
                &mut self.swash_cache,
            )
            .unwrap();
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

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Render Pass"),
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

        self.text_renderer
            .render(&self.atlas, &self.viewport, &mut render_pass)
            .unwrap();
    }
}

fn effective_colors(cell: &Cell) -> (Color, Color) {
    if cell.style & style::REVERSE != 0 {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    }
}

fn build_attrs(cell: &Cell, fg: Color) -> Attrs<'static> {
    // Start with explicit color — never leave color_opt as None
    let mut attrs = Attrs::new().color(fg);

    if cell.style & style::BOLD != 0 {
        attrs = attrs.weight(Weight::BOLD);
    }
    if cell.style & style::ITALIC != 0 {
        attrs = attrs.style(Style::Italic);
    }
    if cell.style & style::DIM != 0 {
        // Halve brightness to approximate dim since glyphon has no dim attr
        let v = fg.0;
        let r = (((v >> 24) & 0xff) as u8) / 2;
        let g = (((v >> 16) & 0xff) as u8) / 2;
        let b = (((v >> 8) & 0xff) as u8) / 2;
        let a = (v & 0xff) as u8;
        attrs = attrs.color(Color::rgba(r, g, b, a));
    }
    if cell.style & style::HIDDEN != 0 {
        // Make text invisible by matching the background
        attrs = attrs.color(cell.bg);
    }

    attrs
}

fn attrs_equal(a: &Attrs, b: &Attrs) -> bool {
    let colors_match = match (a.color_opt, b.color_opt) {
        (Some(ca), Some(cb)) => ca.0 == cb.0,
        (None, None) => true,
        _ => false,
    };
    colors_match && a.weight == b.weight && a.style == b.style
}
