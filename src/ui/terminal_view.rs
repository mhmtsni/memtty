use std::sync::Arc;

use crate::ui::ui::Message;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Theme, Window},
};

use crate::ui::{renderer::Renderer, ui::MyApp};

pub struct TerminalView {
    pub app: Option<MyApp>,
    pub window: Option<Arc<Window>>,
    pub proxy: Option<EventLoopProxy<Message>>,
    pub surface: Option<wgpu::Surface<'static>>,
    pub device: Option<wgpu::Device>,
    pub queue: Option<wgpu::Queue>,
    pub config: Option<wgpu::SurfaceConfiguration>,
    pub terminal_dirty: bool,
    pub redraw_requested: bool,
    pub pending_pty_data: Vec<u8>,
}

impl TerminalView {
    fn request_redraw_if_needed(&mut self) {
        if self.redraw_requested {
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
            self.redraw_requested = true;
        }
    }
}

impl ApplicationHandler<Message> for TerminalView {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            let (blink_changed, blink_deadline) = {
                let app = self.app.as_mut().expect("app checked as some");
                let changed = app.update_cursor_blink();
                (changed, app.next_blink_deadline())
            };

            if blink_changed {
                self.request_redraw_if_needed();
            }

            if let Some(deadline) = blink_deadline {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("memo"))
                .unwrap(),
        );
        window.set_maximized(true);
        window.set_theme(Some(Theme::Dark));

        let (tx_to_pty, rx_from_ui) = tokio::sync::mpsc::unbounded_channel();
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
        let format = surface
            .get_capabilities(&adapter)
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);

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
        let font_size = 25.0;

        surface.configure(&device, &config);

        let renderer = Renderer::new(
            &device,
            &queue,
            format,
            multisample,
            size.width,
            size.height,
            font_size,
        );

        let mut app = MyApp::new(window.clone(), tx_to_pty, renderer);
        if let Some(family) = app.renderer.active_font_family_name() {
            eprintln!("Using terminal font family: {family}");
        } else {
            eprintln!("Using terminal font family: system monospace fallback");
        }
        app.handle_resize(size);

        let proxy = self.proxy.as_ref().unwrap().clone();

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
        self.terminal_dirty = true;
        self.request_redraw_if_needed();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Message) {
        match event {
            Message::PtyDataReceived(data) => {
                self.pending_pty_data.extend_from_slice(&data);
                self.terminal_dirty = true;
                self.request_redraw_if_needed();
            }
            Message::PtyExited => {
                std::process::exit(0);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = match &mut self.app {
            Some(s) => s,
            None => return,
        };

        let mut should_redraw = false;

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::RedrawRequested => {
                self.redraw_requested = false;

                if !self.pending_pty_data.is_empty() {
                    let pending = std::mem::take(&mut self.pending_pty_data);
                    state.terminal.process(&pending);
                    self.terminal_dirty = true;
                }

                if self.terminal_dirty {
                    state.sync_renderer_from_terminal(true);
                    self.terminal_dirty = false;
                }

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

                let fg_color = state.terminal.performer.current_fg;
                state
                    .renderer
                    .render(device, queue, &view, &mut encoder, fg_color);

                queue.submit(Some(encoder.finish()));
                frame.present();
            }

            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(device), Some(config)) =
                        (&self.surface, &self.device, &mut self.config)
                    {
                        config.width = size.width;
                        config.height = size.height;
                        surface.configure(device, config);
                    }
                    state.handle_resize(size);
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
            }
            WindowEvent::Focused(focused) => {
                state.update_has_focus(focused);
                should_redraw = true;
            }

            _ => {}
        }

        if should_redraw {
            self.request_redraw_if_needed();
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
        terminal_dirty: false,
        redraw_requested: false,
        pending_pty_data: Vec::new(),
    };

    event_loop.run_app(&mut app)?;

    Ok(())
}
