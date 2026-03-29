use std::sync::Arc;

use crate::ui::{renderer::Renderer, ui::Message};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::Window,
};

use crate::ui::ui::MyApp;

pub struct TerminalView {
    pub app: Option<MyApp>,
    pub window: Option<Arc<Window>>,
    pub proxy: Option<EventLoopProxy<Message>>,
    pub surface: Option<wgpu::Surface<'static>>,
    pub device: Option<wgpu::Device>,
    pub queue: Option<wgpu::Queue>,
    pub config: Option<wgpu::SurfaceConfiguration>,
}

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
        let instance = wgpu::Instance::default();

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .unwrap();

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();

        let size = window.inner_size();

        let format = surface.get_capabilities(&adapter).formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let multisample = wgpu::MultisampleState::default();

        surface.configure(&device, &config);

        let renderer = Renderer::new(
            &device,
            &queue,
            format,
            multisample,
            size.width,
            size.height,
        );

        // create app
        let app = MyApp::new(window.clone(), tx_to_pty, renderer);

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

        self.window = Some(window.clone());
        self.app = Some(app);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);

        window.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Message) {
        let app = match &mut self.app {
            Some(app) => app,
            None => return,
        };

        match event {
            Message::PtyDataReceived(data) => {
                app.terminal.process(&data);
            }
            Message::PtyExited => {
                std::process::exit(0);
            }
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

        let mut should_redraw = false;

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let surface = self.surface.as_ref().unwrap();
                let device = self.device.as_ref().unwrap();
                let queue = self.queue.as_ref().unwrap();
                let config = self.config.as_ref().unwrap();

                let frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(_) => {
                        surface.configure(device, config);
                        surface.get_current_texture().unwrap()
                    }
                };

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Render Encoder"),
                });

                // 🔥 DRAW TEXT
                self.app
                    .as_mut()
                    .unwrap()
                    .renderer
                    .render(device, queue, &view, &mut encoder);

                queue.submit(Some(encoder.finish()));
                frame.present();
            }
            WindowEvent::Resized(size) => {
                state.handle_resize(size);
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(device), Some(config)) =
                        (&self.surface, &self.device, &mut self.config)
                    {
                        config.width = size.width;
                        config.height = size.height;
                        surface.configure(device, config);
                    }
                    state.renderer.resize(size.width, size.height);
                }
                should_redraw = true;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.set_modifiers(modifiers.state());
            }
            WindowEvent::MouseWheel { delta, .. } => {
                state.handle_mouse_wheel(delta);
                should_redraw = true;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                state.handle_key_event(event);
                should_redraw = true;
            }
            _ => {}
        }

        if should_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let mut app = TerminalView {
        app: None,
        window: None,
        proxy: Some(proxy),
        surface: None,
        device: None,
        queue: None,
        config: None,
    };

    event_loop.run_app(&mut app)?;

    Ok(())
}
