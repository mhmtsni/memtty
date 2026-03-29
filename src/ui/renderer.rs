use glyphon::{
    Attrs, Buffer, Cache, Color, FontSystem, Metrics, Shaping, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

pub struct Renderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    buffer: Buffer,
    pub atlas: TextAtlas,
    pub text_renderer: TextRenderer,

    viewport: Viewport,
    cache: Cache,
    width: u32,
    height: u32,
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

        let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 20.0));

        buffer.set_size(&mut font_system, Some(width as f32), Some(height as f32));

        buffer.set_text(
            &mut font_system,
            "Hello glyphon 0.10",
            &Attrs::new(),
            Shaping::Advanced,
            None::<glyphon::cosmic_text::Align>,
        );

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
            cache,
            width,
            height,
        }
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

    pub fn prepare(&mut self, device: &Device, queue: &Queue) {
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
                    default_color: Color::rgb(255, 255, 255),
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
    ) {
        self.prepare(device, queue);

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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

        self.text_renderer
            .render(&self.atlas, &self.viewport, &mut render_pass)
            .unwrap();
    }
}
