mod kitty;
mod pdf;
mod protocol;
mod video;

use std::io::BufRead;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use kitty::KittyRenderer;
use pdf::{PdfDoc, PdfView};
use protocol::{Command, Event};
use video::VideoDecoder;

fn is_video_path(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v")
}

fn is_pdf_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Viewport in pixels for a window of `cols`x`rows` cells. Falls back to a
/// 1:2 cell when the terminal doesn't report pixel sizes.
fn pdf_viewport(cols: u32, rows: u32) -> (u32, u32) {
    let (cw, ch) = kitty::cell_size_px().unwrap_or((8.0, 16.0));
    pdf::viewport_px(cols, rows, cw, ch)
}

fn emit(event: Event) {
    let json = serde_json::to_string(&event).unwrap();
    println!("{json}");
}

fn emit_time(vs: &mut VideoState) {
    let now = Instant::now();
    if now.duration_since(vs.last_time_emit) >= Duration::from_millis(900) {
        vs.last_time_emit = now;
        emit(Event::Time {
            position: vs.decoder.position_secs(),
            duration: vs.decoder.duration_secs,
            playing: vs.decoder.is_playing(),
        });
    }
}

fn force_emit_time(vs: &mut VideoState) {
    vs.last_time_emit = Instant::now();
    emit(Event::Time {
        position: vs.decoder.position_secs(),
        duration: vs.decoder.duration_secs,
        playing: vs.decoder.is_playing(),
    });
}

struct VideoState {
    decoder: VideoDecoder,
    last_time_emit: Instant,
}

struct PdfState {
    doc: PdfDoc,
    view: PdfView,
}

struct App {
    kitty: KittyRenderer,
    col: u32,
    row: u32,
    width: u32,
    height: u32,
    image: Option<image::DynamicImage>,
    video: Option<VideoState>,
    pdf: Option<PdfState>,
    current_path: Option<String>,
    last_rgba: Option<(Vec<u8>, u32, u32)>,
    last_display_w: u32,
    last_display_h: u32,
}

impl App {
    fn new() -> Self {
        Self {
            kitty: KittyRenderer::new(),
            col: 0,
            row: 0,
            width: 0,
            height: 0,
            image: None,
            video: None,
            pdf: None,
            current_path: None,
            last_rgba: None,
            last_display_w: 0,
            last_display_h: 0,
        }
    }

    /// Returns true if the main loop should exit.
    fn handle(&mut self, cmd: Command) -> Result<bool, String> {
        match cmd {
            Command::Show { path, row, col, width, height } => {
                self.row = row;
                self.col = col;
                self.width = width;
                self.height = height;

                let need_open = self.current_path.as_deref() != Some(path.as_str())
                    || (is_video_path(&path) && self.video.is_none());

                if is_video_path(&path) {
                    if need_open {
                        let decoder = VideoDecoder::open(&path)?;
                        self.current_path = Some(path);
                        self.video = Some(VideoState {
                            decoder,
                            last_time_emit: Instant::now(),
                        });
                        self.image = None;
                        self.pdf = None;
                        self.last_rgba = None;
                        self.kitty.reset();
                        emit(Event::Ready);
                    }
                    if !need_open && (width != self.last_display_w || height != self.last_display_h) {
                        self.kitty.clear()?;
                    }
                    self.last_display_w = width;
                    self.last_display_h = height;
                } else if is_pdf_path(&path) {
                    if need_open {
                        let doc = PdfDoc::open(&path)?;
                        self.current_path = Some(path);
                        self.pdf = Some(PdfState { doc, view: PdfView::default() });
                        self.image = None;
                        self.video = None;
                        self.last_rgba = None;
                    }
                    if !need_open && (width != self.last_display_w || height != self.last_display_h) {
                        self.kitty.clear()?;
                    }
                    self.last_display_w = width;
                    self.last_display_h = height;
                    self.display_pdf()?;
                    emit(Event::Ready);
                    if need_open {
                        self.emit_page();
                    }
                } else {
                    if need_open {
                        let img = image::open(&path)
                            .map_err(|e| format!("failed to open {path}: {e}"))?;
                        self.current_path = Some(path);
                        self.image = Some(img);
                        self.video = None;
                        self.pdf = None;
                        self.kitty.reset();
                    }
                    if !need_open && (width != self.last_display_w || height != self.last_display_h) {
                        self.kitty.clear()?;
                    }
                    self.last_display_w = width;
                    self.last_display_h = height;
                    self.display_image()?;
                    emit(Event::Ready);
                }
                Ok(false)
            }
            Command::Zoom { factor } => {
                if let Some(ps) = self.pdf.as_mut() {
                    ps.view.zoom_by(factor);
                    self.display_pdf()?;
                    return Ok(false);
                }
                self.kitty.zoom_by(factor);
                self.display_image()?;
                Ok(false)
            }
            Command::Reset => {
                if let Some(ps) = self.pdf.as_mut() {
                    ps.view = PdfView::default();
                    self.display_pdf()?;
                    return Ok(false);
                }
                self.kitty.reset();
                self.display_image()?;
                Ok(false)
            }
            Command::Pan { dx, dy } => {
                if let Some(ps) = self.pdf.as_mut() {
                    let (vw, vh) = pdf_viewport(self.width, self.height);
                    let (pw, ph) = ps.doc.current_image()?.dimensions();
                    let moved = ps.view.scroll(dx, dy, pw, ph, vw, vh);
                    // Scrolling past the bottom continues onto the next page's
                    // top; past the top, onto the previous page's bottom.
                    if !moved && dy != 0 && ps.doc.step(dy.signum()) {
                        ps.view.y = if dy > 0 { 0.0 } else { 1.0 };
                        self.display_pdf()?;
                        self.emit_page();
                        return Ok(false);
                    }
                    self.display_pdf()?;
                    return Ok(false);
                }
                self.kitty.pan(dx, dy);
                self.display_image()?;
                Ok(false)
            }
            Command::PlayPause => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.toggle_play();
                    force_emit_time(vs);
                }
                Ok(false)
            }
            Command::Seek { delta } => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.seek(delta)?;
                    force_emit_time(vs);
                }
                Ok(false)
            }
            Command::Rewind => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.rewind()?;
                    force_emit_time(vs);
                }
                Ok(false)
            }
            Command::Page { delta } => {
                let changed = self.pdf.as_mut().is_some_and(|ps| ps.doc.step(delta));
                if changed { self.on_page_changed()?; }
                Ok(false)
            }
            Command::PageGoto { page } => {
                let changed = self.pdf.as_mut().is_some_and(|ps| ps.doc.goto(page));
                if changed { self.on_page_changed()?; }
                Ok(false)
            }
            Command::Clear => {
                self.kitty.clear()?;
                Ok(false)
            }
            Command::Quit => {
                let _ = self.kitty.clear();
                Ok(true)
            }
        }
    }

    /// New page starts at the top; zoom and horizontal position carry over.
    fn on_page_changed(&mut self) -> Result<(), String> {
        if let Some(ps) = self.pdf.as_mut() {
            ps.view.y = 0.0;
        }
        self.display_pdf()?;
        self.emit_page();
        Ok(())
    }

    fn emit_page(&self) {
        if let Some(ps) = self.pdf.as_ref() {
            emit(Event::Page { page: ps.doc.page, count: ps.doc.page_count });
        }
    }

    fn display_pdf(&mut self) -> Result<(), String> {
        let Some(ps) = self.pdf.as_mut() else { return Ok(()) };
        let (vw, vh) = pdf_viewport(self.width, self.height);
        let frame = pdf::compose(ps.doc.current_image()?, &ps.view, vw, vh);
        self.kitty.display_raw(frame.as_raw(), vw, vh, self.col, self.row, self.width, self.height)
    }

    fn display_image(&mut self) -> Result<(), String> {
        if let Some(ref img) = self.image {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            self.kitty.display(rgba.as_raw(), w, h, self.col, self.row, self.width, self.height)?;
        } else if let Some((ref rgba, w, h)) = self.last_rgba {
            self.kitty.display(rgba, w, h, self.col, self.row, self.width, self.height)?;
        }
        Ok(())
    }
}

fn main() {
    let (tx, rx) = mpsc::channel::<Command>();

    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() { continue; }
            match serde_json::from_str(&line) {
                Ok(cmd) => { if tx.send(cmd).is_err() { break; } }
                Err(e) => eprintln!("nvim-gfx: bad command: {e}: {line}"),
            }
        }
    });

    let mut app = App::new();

    // Cap how often we transmit a video frame to the terminal. The terminal —
    // not decoding — is the bottleneck: it re-parses and re-uploads a full frame
    // every transmit. Decoding still tracks the source rate (playback position
    // stays correct); we just skip painting frames that fall between ticks. Tune
    // MAX_DRAW_FPS to trade smoothness for terminal CPU.
    const MAX_DRAW_FPS: u64 = 15;
    let min_draw_interval = Duration::from_micros(1_000_000 / MAX_DRAW_FPS);
    let mut last_draw = Instant::now() - Duration::from_secs(1);

    loop {
        // Drain all pending commands before checking video frame.
        loop {
            match rx.try_recv() {
                Ok(cmd) => match app.handle(cmd) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(e) => { emit(Event::Error { msg: e }); return; }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = app.kitty.clear();
                    return;
                }
            }
        }

        let active = app.video.as_ref()
            .map(|vs| vs.decoder.is_playing() && !vs.decoder.is_finished())
            .unwrap_or(false);

        if active {
            let now = Instant::now();
            let display_time = app.video.as_ref().unwrap().decoder.current_display_time();
            if now >= display_time {
                let (w, h) = {
                    let vs = app.video.as_ref().unwrap();
                    (vs.decoder.width(), vs.decoder.height())
                };
                loop {
                    match app.video.as_mut().unwrap().decoder.next_frame() {
                        Ok(Some(rgba)) => {
                            let frame_dt = app.video.as_ref().unwrap().decoder.current_display_time();
                            if Instant::now() >= frame_dt {
                                continue;
                            }
                            app.last_rgba = Some((rgba.clone(), w, h));
                            // Only paint if enough time has passed since the last
                            // transmit (terminal-rate cap). Skipped frames still
                            // advanced playback above; we just don't send them.
                            if Instant::now().duration_since(last_draw) >= min_draw_interval {
                                let (col, row, width, height) =
                                    (app.col, app.row, app.width, app.height);
                                if let Err(e) = app.kitty.display(&rgba, w, h, col, row, width, height) {
                                    emit(Event::Error { msg: e });
                                    return;
                                }
                                last_draw = Instant::now();
                            }
                            emit_time(app.video.as_mut().unwrap());
                            break;
                        }
                        Ok(None) => { break; }
                        Err(e) => { emit(Event::Error { msg: e }); return; }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        } else {
            match rx.recv() {
                Ok(cmd) => match app.handle(cmd) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(e) => { emit(Event::Error { msg: e }); return; }
                },
                Err(_) => {
                    let _ = app.kitty.clear();
                    return;
                }
            }
        }
    }
}
