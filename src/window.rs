use std::num::NonZeroU32;
use std::sync::Arc;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

use crate::geometry::{self, PixelGeometry};
use crate::protocol::{Command, Event};
use crate::renderer::Renderer;

pub struct App {
    window: Option<Arc<Window>>,
    context: Option<Context<Arc<Window>>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    renderer: Renderer,
    cell_w: u32,
    cell_h: u32,
}

impl App {
    pub fn new() -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            renderer: Renderer::new(),
            cell_w: 8,
            cell_h: 16,
        }
    }

    fn ensure_window(&mut self, event_loop: &ActiveEventLoop, geo: &PixelGeometry) {
        if let Some(ref win) = self.window {
            win.set_outer_position(PhysicalPosition::new(geo.x, geo.y));
            let _ = win.request_inner_size(PhysicalSize::new(geo.width, geo.height));
        } else {
            let attrs = Window::default_attributes()
                .with_decorations(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_position(PhysicalPosition::new(geo.x, geo.y))
                .with_inner_size(PhysicalSize::new(geo.width, geo.height));

            match event_loop.create_window(attrs) {
                Ok(win) => {
                    let win = Arc::new(win);
                    let ctx = Context::new(win.clone()).expect("softbuffer context");
                    let surf = Surface::new(&ctx, win.clone()).expect("softbuffer surface");
                    self.context = Some(ctx);
                    self.surface = Some(surf);
                    self.window = Some(win);
                }
                Err(e) => {
                    emit(Event::Error { msg: format!("window creation failed: {e}") });
                    event_loop.exit();
                }
            }
        }
    }

    fn request_redraw(&self) {
        if let Some(ref win) = self.window {
            win.request_redraw();
        }
    }

    fn do_render(&mut self) {
        let (Some(ref win), Some(ref mut surf)) = (&self.window, &mut self.surface) else {
            return;
        };
        let size = win.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        surf.resize(
            NonZeroU32::new(size.width).unwrap(),
            NonZeroU32::new(size.height).unwrap(),
        )
        .expect("surface resize");
        let mut buf = surf.buffer_mut().expect("buffer_mut");
        self.renderer.render(&mut buf, size.width, size.height);
        buf.present().expect("present");
        win.set_visible(true);
    }
}

impl ApplicationHandler<Command> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            self.do_render();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, cmd: Command) {
        match cmd {
            Command::Show { path, x, y, w, h, cols, rows } => {
                match geometry::compute_geometry(x, y, w, h, cols, rows) {
                    Ok(geo) => {
                        if w > 0 { self.cell_w = geo.width / w; }
                        if h > 0 { self.cell_h = geo.height / h; }
                        self.ensure_window(event_loop, &geo);
                        match self.renderer.load(&path) {
                            Ok(()) => {
                                self.request_redraw();
                                emit(Event::Ready);
                            }
                            Err(msg) => {
                                emit(Event::Error { msg });
                                event_loop.exit();
                            }
                        }
                    }
                    Err(msg) => {
                        emit(Event::Error { msg });
                        event_loop.exit();
                    }
                }
            }
            Command::Zoom { factor } => {
                self.renderer.zoom(factor);
                self.request_redraw();
            }
            Command::Pan { dx, dy } => {
                self.renderer.pan(dx as f32 * self.cell_w as f32, dy as f32 * self.cell_h as f32);
                self.request_redraw();
            }
            Command::Reset => {
                self.renderer.reset();
                self.request_redraw();
            }
            Command::Quit => {
                event_loop.exit();
            }
        }
    }
}

pub fn emit(event: Event) {
    let json = serde_json::to_string(&event).unwrap();
    println!("{json}");
}
