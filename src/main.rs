mod kitty;
mod protocol;
mod video;

use std::io::BufRead;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use kitty::{xrgb_to_rgba, KittyRenderer};
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

fn emit(event: Event) {
    let json = serde_json::to_string(&event).unwrap();
    println!("{json}");
}

struct VideoState {
    decoder: VideoDecoder,
    next_frame_time: Instant,
}

struct App {
    kitty: KittyRenderer,
    col: u32,
    row: u32,
    width: u32,
    height: u32,
    image: Option<image::DynamicImage>,
    video: Option<VideoState>,
    current_path: Option<String>,
    last_rgba: Option<(Vec<u8>, u32, u32)>,
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
            current_path: None,
            last_rgba: None,
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
                            next_frame_time: Instant::now(),
                        });
                        self.image = None;
                        self.last_rgba = None;
                        self.kitty.reset();
                        emit(Event::Ready);
                    }
                    // On resize: width/height updated above; next frame renders at new size.
                } else {
                    if need_open {
                        let img = image::open(&path)
                            .map_err(|e| format!("failed to open {path}: {e}"))?;
                        self.current_path = Some(path);
                        self.image = Some(img);
                        self.video = None;
                        self.kitty.reset();
                    }
                    self.display_image()?;
                    emit(Event::Ready);
                }
                Ok(false)
            }
            Command::Zoom { factor } => {
                self.kitty.zoom_by(factor);
                self.display_image()?;
                Ok(false)
            }
            Command::Pan { dx, dy } => {
                self.kitty.pan(dx, dy);
                self.display_image()?;
                Ok(false)
            }
            Command::Reset => {
                self.kitty.reset();
                self.display_image()?;
                Ok(false)
            }
            Command::PlayPause => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.toggle_play();
                    if vs.decoder.is_playing() {
                        vs.next_frame_time = Instant::now();
                    }
                }
                Ok(false)
            }
            Command::Seek { delta } => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.seek(delta)?;
                    vs.next_frame_time = Instant::now();
                }
                Ok(false)
            }
            Command::Rewind => {
                if let Some(ref mut vs) = self.video {
                    vs.decoder.rewind()?;
                    vs.next_frame_time = Instant::now();
                }
                Ok(false)
            }
            Command::Quit => {
                let _ = self.kitty.clear();
                Ok(true)
            }
        }
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
                Err(TryRecvError::Disconnected) => return,
            }
        }

        let active = app.video.as_ref()
            .map(|vs| vs.decoder.is_playing() && !vs.decoder.is_finished())
            .unwrap_or(false);

        if active {
            let now = Instant::now();
            let next = app.video.as_ref().unwrap().next_frame_time;
            if now >= next {
                let (w, h) = {
                    let vs = app.video.as_ref().unwrap();
                    (vs.decoder.width(), vs.decoder.height())
                };
                match app.video.as_mut().unwrap().decoder.next_frame() {
                    Ok(Some(pixels)) => {
                        let rgba = xrgb_to_rgba(&pixels);
                        app.last_rgba = Some((rgba.clone(), w, h));
                        let (col, row, width, height) =
                            (app.col, app.row, app.width, app.height);
                        if let Err(e) = app.kitty.display(&rgba, w, h, col, row, width, height) {
                            emit(Event::Error { msg: e });
                            return;
                        }
                        let dur = app.video.as_ref().unwrap().decoder.frame_duration();
                        app.video.as_mut().unwrap().next_frame_time = now + dur;
                    }
                    Ok(None) => {}
                    Err(e) => { emit(Event::Error { msg: e }); return; }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        } else {
            // No active video — block until a command arrives.
            match rx.recv() {
                Ok(cmd) => match app.handle(cmd) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(e) => { emit(Event::Error { msg: e }); return; }
                },
                Err(_) => return,
            }
        }
    }
}
