use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId, WindowLevel};
#[cfg(target_os = "linux")]
use winit::platform::x11::WindowAttributesExtX11;

use crate::geometry::{self, PixelGeometry};
use crate::protocol::{Command, Event};
use crate::renderer::Renderer;
use crate::video::VideoDecoder;

struct VideoState {
    decoder: VideoDecoder,
    next_frame_time: Instant,
}

pub struct App {
    window: Option<Arc<Window>>,
    context: Option<Context<Arc<Window>>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    renderer: Renderer,
    cell_w: u32,
    cell_h: u32,
    video: Option<VideoState>,
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
            video: None,
        }
    }

    fn ensure_window(&mut self, event_loop: &ActiveEventLoop, geo: &PixelGeometry) {
        if let Some(ref win) = self.window {
            win.set_outer_position(PhysicalPosition::new(geo.x, geo.y));
            let _ = win.request_inner_size(PhysicalSize::new(geo.width, geo.height));
        } else {
            #[allow(unused_mut)]
            let mut attrs = Window::default_attributes()
                .with_decorations(false)
                .with_visible(false)
                .with_active(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_position(PhysicalPosition::new(geo.x, geo.y))
                .with_inner_size(PhysicalSize::new(geo.width, geo.height));
            #[cfg(target_os = "linux")]
            { attrs = attrs.with_override_redirect(true); }

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

fn is_video_path(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v")
}

impl ApplicationHandler<Command> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            self.do_render();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Collect what we need from VideoState in a scoped borrow, then release
        // before calling self.renderer (which also borrows self mutably).
        let frame_data: Option<Result<Option<Vec<u32>>, String>> = {
            let Some(ref mut vs) = self.video else { return };
            if !vs.decoder.is_playing() || vs.decoder.is_finished() {
                return;
            }
            let now = Instant::now();
            if now < vs.next_frame_time {
                None // not yet time for the next frame
            } else {
                let result = vs.decoder.next_frame();
                let frame_dur = vs.decoder.frame_duration();
                vs.next_frame_time = now + frame_dur;
                Some(result)
            }
        };

        match frame_data {
            Some(Ok(Some(pixels))) => {
                let (w, h) = self.video.as_ref().map(|vs| {
                    (vs.decoder.width(), vs.decoder.height())
                }).unwrap_or((0, 0));
                self.renderer.update_frame(pixels, w, h);
                self.request_redraw();
            }
            Some(Ok(None)) => {
                // End of video — last frame stays on screen
            }
            Some(Err(msg)) => {
                emit(Event::Error { msg });
                event_loop.exit();
                return;
            }
            None => {}
        }

        if let Some(ref vs) = self.video {
            event_loop.set_control_flow(ControlFlow::WaitUntil(vs.next_frame_time));
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

                        if is_video_path(&path) {
                            match VideoDecoder::open(&path) {
                                Ok(decoder) => {
                                    let frame_dur = decoder.frame_duration();
                                    self.video = Some(VideoState {
                                        decoder,
                                        next_frame_time: Instant::now() + frame_dur,
                                    });
                                    emit(Event::Ready);
                                }
                                Err(msg) => {
                                    emit(Event::Error { msg });
                                    event_loop.exit();
                                }
                            }
                        } else {
                            self.video = None;
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
                self.renderer.pan(
                    dx as f32 * self.cell_w as f32,
                    dy as f32 * self.cell_h as f32,
                );
                self.request_redraw();
            }
            Command::Reset => {
                self.renderer.reset();
                self.request_redraw();
            }
            Command::PlayPause => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.toggle_play();
                    if vs.decoder.is_playing() {
                        vs.next_frame_time = Instant::now();
                    }
                }
            }
            Command::Seek { delta } => {
                if let Some(ref mut vs) = self.video {
                    if let Err(msg) = vs.decoder.seek(delta) {
                        emit(Event::Error { msg });
                    } else {
                        vs.next_frame_time = Instant::now();
                    }
                }
            }
            Command::Rewind => {
                if let Some(ref mut vs) = self.video {
                    if let Err(msg) = vs.decoder.rewind() {
                        emit(Event::Error { msg });
                    } else {
                        vs.next_frame_time = Instant::now();
                    }
                }
            }
            Command::Hide => {
                if let Some(ref win) = self.window {
                    win.set_visible(false);
                }
            }
            Command::Unhide => {
                if let Some(ref win) = self.window {
                    win.set_visible(true);
                    self.request_redraw();
                }
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
