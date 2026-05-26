# nvim-gfx Kitty Graphics Protocol Rewrite

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken X11 overlay rendering backend with the Kitty graphics protocol — the Rust binary writes Kitty escape sequences to `/dev/tty` instead of managing an X11 window.

**Architecture:** Rust binary decodes images/video, applies zoom/pan as a crop on raw RGBA pixels, and writes Kitty protocol escape sequences directly to `/dev/tty`. Lua sends JSON commands via stdin exactly as before; only the geometry values change from pixel coords to cell coords.

**Tech Stack:** Rust (ffmpeg-next, image, base64, serde_json), Lua (Neovim API), Kitty graphics protocol

**Spec:** `docs/superpowers/specs/2026-05-26-kitty-rewrite-design.md`

---

## File Map

| File | Action |
|------|--------|
| `src/protocol.rs` | Modify — new Show fields, remove Hide/Unhide |
| `Cargo.toml` | Modify — remove winit/softbuffer/x11rb/libc, add base64 |
| `src/kitty.rs` | Create — KittyRenderer view state + Kitty protocol encoder |
| `src/main.rs` | Rewrite — no winit, stdin thread + main state loop |
| `src/window.rs` | Delete |
| `src/renderer.rs` | Delete |
| `src/geometry.rs` | Delete |
| `scripts/xvfb-test.py` | Delete — X11-specific, replaced by updated test.sh |
| `lua/nvim-gfx/geometry.lua` | Rewrite — cell coordinates only |
| `lua/nvim-gfx/viewer.lua` | Modify — new show command format |
| `scripts/test.sh` | Modify — remove xvfb, test new protocol |

---

### Task 1: Update src/protocol.rs

**Files:**
- Modify: `src/protocol.rs`

- [ ] **Step 1: Write the failing tests**

Add to `src/protocol.rs` `#[cfg(test)]` block:

```rust
#[test]
fn deserialize_show_cell_coords() {
    let json = r#"{"cmd":"show","path":"/tmp/a.png","row":2,"col":5,"width":80,"height":24}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::Show { path, row, col, width, height } => {
            assert_eq!(path, "/tmp/a.png");
            assert_eq!(row, 2);
            assert_eq!(col, 5);
            assert_eq!(width, 80);
            assert_eq!(height, 24);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn hide_unhide_do_not_exist() {
    // Show that the old pixel-based Show is rejected
    let json = r#"{"cmd":"show","path":"/tmp/a.png","x":0,"y":0,"w":80,"h":24,"cols":200,"rows":50}"#;
    assert!(serde_json::from_str::<Command>(&json).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p nvim-gfx 2>&1 | grep -E "FAILED|error|deserialize_show_cell"
```

Expected: `deserialize_show_cell_coords` fails (old Show struct doesn't have `row`/`col`/`width`/`height`).

- [ ] **Step 3: Replace the Command enum**

Replace the entire contents of `src/protocol.rs` with:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Show { path: String, row: u32, col: u32, width: u32, height: u32 },
    Zoom { factor: f32 },
    Pan { dx: i32, dy: i32 },
    Reset,
    PlayPause,
    Seek { delta: i32 },
    Rewind,
    Quit,
}

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Ready,
    Error { msg: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_show_cell_coords() {
        let json = r#"{"cmd":"show","path":"/tmp/a.png","row":2,"col":5,"width":80,"height":24}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Show { path, row, col, width, height } => {
                assert_eq!(path, "/tmp/a.png");
                assert_eq!(row, 2);
                assert_eq!(col, 5);
                assert_eq!(width, 80);
                assert_eq!(height, 24);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn hide_unhide_do_not_exist() {
        let json = r#"{"cmd":"show","path":"/tmp/a.png","x":0,"y":0,"w":80,"h":24,"cols":200,"rows":50}"#;
        assert!(serde_json::from_str::<Command>(&json).is_err());
    }

    #[test]
    fn deserialize_zoom() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"zoom","factor":1.25}"#).unwrap();
        match cmd {
            Command::Zoom { factor } => assert!((factor - 1.25).abs() < 0.001),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_pan() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"pan","dx":-1,"dy":0}"#).unwrap();
        match cmd {
            Command::Pan { dx, dy } => { assert_eq!(dx, -1); assert_eq!(dy, 0); }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_reset() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"reset"}"#).unwrap();
        assert!(matches!(cmd, Command::Reset));
    }

    #[test]
    fn deserialize_quit() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"quit"}"#).unwrap();
        assert!(matches!(cmd, Command::Quit));
    }

    #[test]
    fn serialize_ready() {
        let json = serde_json::to_string(&Event::Ready).unwrap();
        assert_eq!(json, r#"{"event":"ready"}"#);
    }

    #[test]
    fn serialize_error() {
        let json = serde_json::to_string(&Event::Error { msg: "oops".into() }).unwrap();
        assert_eq!(json, r#"{"event":"error","msg":"oops"}"#);
    }

    #[test]
    fn deserialize_play_pause() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"play_pause"}"#).unwrap();
        assert!(matches!(cmd, Command::PlayPause));
    }

    #[test]
    fn deserialize_seek() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"seek","delta":-10}"#).unwrap();
        match cmd {
            Command::Seek { delta } => assert_eq!(delta, -10),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserialize_rewind() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"rewind"}"#).unwrap();
        assert!(matches!(cmd, Command::Rewind));
    }
}
```

- [ ] **Step 4: Run tests — expect compile errors from other files using old Show**

```bash
cargo test 2>&1 | head -30
```

Expected: compile errors in `src/window.rs` referencing old Show fields (`x`, `y`, `w`, `h`, `cols`, `rows`). This is expected — Task 4 rewrites main.rs which removes the window module.

- [ ] **Step 5: Run protocol tests in isolation**

```bash
cargo test --lib protocol 2>&1
```

Expected: all `protocol::tests` pass.

- [ ] **Step 6: Commit**

```bash
git add src/protocol.rs
git commit -m "refactor: update Show command to cell coords, remove Hide/Unhide"
```

---

### Task 2: Update Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Replace Cargo.toml dependencies**

Replace the `[dependencies]` section with:

```toml
[dependencies]
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "webp"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ffmpeg-next = "6"
base64 = "0.22"
```

The full `Cargo.toml` should be:

```toml
[package]
name = "nvim-gfx"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "nvim-gfx"
path = "src/main.rs"

[dependencies]
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "webp"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ffmpeg-next = "6"
base64 = "0.22"
```

- [ ] **Step 2: Verify base64 resolves**

```bash
cargo fetch 2>&1
```

Expected: `base64 v0.22.x` appears in fetch output (or "Finished" if already cached).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove winit/softbuffer/x11rb/libc, add base64"
```

---

### Task 3: Create src/kitty.rs

**Files:**
- Create: `src/kitty.rs`

This module owns view state (zoom, pan) and writes Kitty graphics protocol escape sequences to `/dev/tty`.

- [ ] **Step 1: Write the failing tests first**

Create `src/kitty.rs` with only the tests (no implementation yet):

```rust
pub struct KittyRenderer {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl KittyRenderer {
    pub fn new() -> Self { todo!() }
    pub fn zoom_by(&mut self, _factor: f32) { todo!() }
    pub fn pan(&mut self, _dx: i32, _dy: i32) { todo!() }
    pub fn reset(&mut self) { todo!() }
    pub fn display(&self, _pixels: &[u8], _src_w: u32, _src_h: u32, _col: u32, _row: u32, _dest_cols: u32, _dest_rows: u32) -> Result<(), String> { todo!() }
    pub fn clear() -> Result<(), String> { todo!() }
    fn crop(&self, _pixels: &[u8], _w: u32, _h: u32) -> (u32, u32, Vec<u8>) { todo!() }
}

pub fn xrgb_to_rgba(_pixels: &[u32]) -> Vec<u8> { todo!() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let r = KittyRenderer::new();
        assert!((r.zoom - 1.0).abs() < 0.001);
        assert_eq!(r.pan_x, 0.0);
        assert_eq!(r.pan_y, 0.0);
    }

    #[test]
    fn zoom_multiplies() {
        let mut r = KittyRenderer::new();
        r.zoom_by(2.0);
        r.zoom_by(1.5);
        assert!((r.zoom - 3.0).abs() < 0.001);
    }

    #[test]
    fn zoom_clamped_min() {
        let mut r = KittyRenderer::new();
        r.zoom_by(0.00001);
        assert!(r.zoom >= 0.05);
    }

    #[test]
    fn zoom_clamped_max() {
        let mut r = KittyRenderer::new();
        r.zoom_by(99999.0);
        assert!(r.zoom <= 32.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut r = KittyRenderer::new();
        r.zoom_by(3.0);
        r.pan(5, 3);
        r.reset();
        assert!((r.zoom - 1.0).abs() < 0.001);
        assert_eq!(r.pan_x, 0.0);
        assert_eq!(r.pan_y, 0.0);
    }

    #[test]
    fn pan_accumulates() {
        let mut r = KittyRenderer::new();
        r.pan(2, 1);
        assert!((r.pan_x - 0.2).abs() < 0.001);
        assert!((r.pan_y - 0.1).abs() < 0.001);
    }

    #[test]
    fn pan_clamped() {
        let mut r = KittyRenderer::new();
        for _ in 0..20 { r.pan(1, 1); }
        assert!(r.pan_x <= 0.9);
        assert!(r.pan_y <= 0.9);
    }

    #[test]
    fn crop_no_zoom_no_pan_passthrough() {
        let r = KittyRenderer::new();
        let pixels = vec![255u8; 4]; // 1x1 RGBA
        let (w, h, out) = r.crop(&pixels, 1, 1);
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(out, pixels);
    }

    #[test]
    fn crop_zoom2_halves_visible_area() {
        let mut r = KittyRenderer::new();
        r.zoom_by(2.0);
        let pixels = vec![1u8; 4 * 4 * 4]; // 4x4 RGBA
        let (w, h, out) = r.crop(&pixels, 4, 4);
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(out.len(), 2 * 2 * 4);
    }

    #[test]
    fn crop_zoom_never_zero_dimension() {
        let mut r = KittyRenderer::new();
        r.zoom_by(99999.0); // extreme zoom
        let pixels = vec![1u8; 4]; // 1x1
        let (w, h, _) = r.crop(&pixels, 1, 1);
        assert!(w >= 1);
        assert!(h >= 1);
    }

    #[test]
    fn xrgb_to_rgba_converts_correctly() {
        let xrgb = vec![0x00FF8040u32];
        let rgba = xrgb_to_rgba(&xrgb);
        assert_eq!(rgba, vec![255, 128, 64, 255]);
    }

    #[test]
    fn xrgb_to_rgba_black() {
        let rgba = xrgb_to_rgba(&[0u32]);
        assert_eq!(rgba, vec![0, 0, 0, 255]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (todo! panics)**

```bash
cargo test --lib kitty 2>&1 | grep -E "FAILED|panicked|ok"
```

Expected: tests fail with `not yet implemented` panics.

- [ ] **Step 3: Implement KittyRenderer**

Replace the contents of `src/kitty.rs` with the full implementation:

```rust
use std::fs::OpenOptions;
use std::io::Write;
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub struct KittyRenderer {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl KittyRenderer {
    pub fn new() -> Self {
        Self { zoom: 1.0, pan_x: 0.0, pan_y: 0.0 }
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.05, 32.0);
    }

    /// dx/dy are cell counts; each unit pans 10% of the visible area.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        self.pan_x = (self.pan_x + dx as f32 * 0.1).clamp(-0.9, 0.9);
        self.pan_y = (self.pan_y + dy as f32 * 0.1).clamp(-0.9, 0.9);
    }

    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }

    /// Write image pixels to the terminal at the given cell position.
    /// pixels: RGBA, 4 bytes per pixel, row-major.
    /// col/row: 0-indexed terminal cell coordinates of the top-left corner.
    /// dest_cols/dest_rows: how many terminal cells the image should fill.
    pub fn display(
        &self,
        pixels: &[u8],
        src_w: u32,
        src_h: u32,
        col: u32,
        row: u32,
        dest_cols: u32,
        dest_rows: u32,
    ) -> Result<(), String> {
        let (crop_w, crop_h, crop_data) = self.crop(pixels, src_w, src_h);
        write_kitty(&crop_data, crop_w, crop_h, col, row, dest_cols, dest_rows)
    }

    /// Remove the current Kitty image placement.
    pub fn clear() -> Result<(), String> {
        let mut tty = open_tty()?;
        write!(tty, "\x1b_Ga=d,d=i,i=1\x1b\\").map_err(|e| format!("tty write: {e}"))?;
        tty.flush().map_err(|e| format!("tty flush: {e}"))
    }

    fn crop(&self, pixels: &[u8], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
        let visible_w = ((w as f32 / self.zoom).round() as u32).clamp(1, w);
        let visible_h = ((h as f32 / self.zoom).round() as u32).clamp(1, h);

        let max_cx = (w - visible_w) as f32;
        let max_cy = (h - visible_h) as f32;
        let cx = (max_cx / 2.0 + self.pan_x * visible_w as f32)
            .round()
            .clamp(0.0, max_cx) as u32;
        let cy = (max_cy / 2.0 + self.pan_y * visible_h as f32)
            .round()
            .clamp(0.0, max_cy) as u32;

        if visible_w == w && visible_h == h && cx == 0 && cy == 0 {
            return (w, h, pixels.to_vec());
        }

        let mut out = Vec::with_capacity((visible_w * visible_h * 4) as usize);
        for y in cy..cy + visible_h {
            let row_start = ((y * w + cx) * 4) as usize;
            out.extend_from_slice(&pixels[row_start..row_start + (visible_w * 4) as usize]);
        }
        (visible_w, visible_h, out)
    }
}

/// Convert XRGB u32 pixels (0x00RRGGBB) to RGBA u8 bytes (alpha = 255).
pub fn xrgb_to_rgba(pixels: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for &p in pixels {
        out.push(((p >> 16) & 0xFF) as u8);
        out.push(((p >> 8) & 0xFF) as u8);
        out.push((p & 0xFF) as u8);
        out.push(255);
    }
    out
}

fn open_tty() -> Result<std::fs::File, String> {
    OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .map_err(|e| format!("open /dev/tty: {e}"))
}

fn write_kitty(
    pixels: &[u8],
    src_w: u32,
    src_h: u32,
    col: u32,
    row: u32,
    dest_cols: u32,
    dest_rows: u32,
) -> Result<(), String> {
    let mut tty = open_tty()?;

    write!(tty, "\x1b[{};{}H", row + 1, col + 1)
        .map_err(|e| format!("cursor position: {e}"))?;

    let b64 = STANDARD.encode(pixels);
    let b64_bytes = b64.as_bytes();
    let total_chunks = (b64_bytes.len() + 4095) / 4096;

    for (i, chunk) in b64_bytes.chunks(4096).enumerate() {
        let chunk_str = std::str::from_utf8(chunk).unwrap();
        let m = if i + 1 < total_chunks { 1 } else { 0 };
        if i == 0 {
            write!(
                tty,
                "\x1b_Ga=T,f=32,s={src_w},v={src_h},c={dest_cols},r={dest_rows},i=1,q=2,m={m};{chunk_str}\x1b\\"
            )
        } else {
            write!(tty, "\x1b_Gm={m};{chunk_str}\x1b\\")
        }
        .map_err(|e| format!("kitty write chunk {i}: {e}"))?;
    }

    tty.flush().map_err(|e| format!("tty flush: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let r = KittyRenderer::new();
        assert!((r.zoom - 1.0).abs() < 0.001);
        assert_eq!(r.pan_x, 0.0);
        assert_eq!(r.pan_y, 0.0);
    }

    #[test]
    fn zoom_multiplies() {
        let mut r = KittyRenderer::new();
        r.zoom_by(2.0);
        r.zoom_by(1.5);
        assert!((r.zoom - 3.0).abs() < 0.001);
    }

    #[test]
    fn zoom_clamped_min() {
        let mut r = KittyRenderer::new();
        r.zoom_by(0.00001);
        assert!(r.zoom >= 0.05);
    }

    #[test]
    fn zoom_clamped_max() {
        let mut r = KittyRenderer::new();
        r.zoom_by(99999.0);
        assert!(r.zoom <= 32.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut r = KittyRenderer::new();
        r.zoom_by(3.0);
        r.pan(5, 3);
        r.reset();
        assert!((r.zoom - 1.0).abs() < 0.001);
        assert_eq!(r.pan_x, 0.0);
        assert_eq!(r.pan_y, 0.0);
    }

    #[test]
    fn pan_accumulates() {
        let mut r = KittyRenderer::new();
        r.pan(2, 1);
        assert!((r.pan_x - 0.2).abs() < 0.001);
        assert!((r.pan_y - 0.1).abs() < 0.001);
    }

    #[test]
    fn pan_clamped() {
        let mut r = KittyRenderer::new();
        for _ in 0..20 { r.pan(1, 1); }
        assert!(r.pan_x <= 0.9);
        assert!(r.pan_y <= 0.9);
    }

    #[test]
    fn crop_no_zoom_no_pan_passthrough() {
        let r = KittyRenderer::new();
        let pixels = vec![255u8; 4];
        let (w, h, out) = r.crop(&pixels, 1, 1);
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(out, pixels);
    }

    #[test]
    fn crop_zoom2_halves_visible_area() {
        let mut r = KittyRenderer::new();
        r.zoom_by(2.0);
        let pixels = vec![1u8; 4 * 4 * 4];
        let (w, h, out) = r.crop(&pixels, 4, 4);
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(out.len(), 2 * 2 * 4);
    }

    #[test]
    fn crop_zoom_never_zero_dimension() {
        let mut r = KittyRenderer::new();
        r.zoom_by(99999.0);
        let pixels = vec![1u8; 4];
        let (w, h, _) = r.crop(&pixels, 1, 1);
        assert!(w >= 1);
        assert!(h >= 1);
    }

    #[test]
    fn xrgb_to_rgba_converts_correctly() {
        let xrgb = vec![0x00FF8040u32];
        let rgba = xrgb_to_rgba(&xrgb);
        assert_eq!(rgba, vec![255, 128, 64, 255]);
    }

    #[test]
    fn xrgb_to_rgba_black() {
        let rgba = xrgb_to_rgba(&[0u32]);
        assert_eq!(rgba, vec![0, 0, 0, 255]);
    }
}
```

- [ ] **Step 4: Run kitty tests**

```bash
cargo test --lib kitty 2>&1
```

Expected:
```
test kitty::tests::crop_no_zoom_no_pan_passthrough ... ok
test kitty::tests::crop_zoom2_halves_visible_area ... ok
test kitty::tests::crop_zoom_never_zero_dimension ... ok
test kitty::tests::initial_state ... ok
test kitty::tests::pan_accumulates ... ok
test kitty::tests::pan_clamped ... ok
test kitty::tests::reset_clears_state ... ok
test kitty::tests::xrgb_to_rgba_black ... ok
test kitty::tests::xrgb_to_rgba_converts_correctly ... ok
test kitty::tests::zoom_clamped_max ... ok
test kitty::tests::zoom_clamped_min ... ok
test kitty::tests::zoom_multiplies ... ok
```

- [ ] **Step 5: Commit**

```bash
git add src/kitty.rs
git commit -m "feat: add kitty.rs — Kitty protocol encoder with view state"
```

---

### Task 4: Rewrite src/main.rs

**Files:**
- Modify: `src/main.rs`

This removes the winit event loop and replaces it with a plain stdin thread + main loop.

- [ ] **Step 1: Replace src/main.rs**

```rust
mod kitty;
mod protocol;
mod video;

use std::io::BufRead;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use image::GenericImageView as _;
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
                let _ = KittyRenderer::clear();
                Ok(true)
            }
        }
    }

    fn display_image(&self) -> Result<(), String> {
        if let Some(ref img) = self.image {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            self.kitty.display(rgba.as_raw(), w, h, self.col, self.row, self.width, self.height)?;
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
```

- [ ] **Step 2: Build (expect link errors from old module references)**

```bash
cargo build 2>&1 | head -20
```

Expected: compile errors about `mod window` and `mod renderer` and `mod geometry` not found — those files still exist but are no longer referenced. Actually they WILL be compiled since they're declared in the old main.rs… but we replaced main.rs so they are no longer declared. The build may warn about unused files but should not error. If there are errors about missing modules, proceed to Step 3.

- [ ] **Step 3: Build release and verify clean**

```bash
cargo build --release 2>&1
```

Expected: `Finished release profile` with no errors. Warnings about unused code in old files are OK.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: rewrite main.rs — drop winit, stdin thread + main loop"
```

---

### Task 5: Remove X11 Files

**Files:**
- Delete: `src/window.rs`, `src/renderer.rs`, `src/geometry.rs`, `scripts/xvfb-test.py`

- [ ] **Step 1: Delete the files**

```bash
rm src/window.rs src/renderer.rs src/geometry.rs scripts/xvfb-test.py
```

- [ ] **Step 2: Build to confirm nothing broke**

```bash
cargo build --release 2>&1
```

Expected: `Finished release profile` with no errors.

- [ ] **Step 3: Run all Rust tests**

```bash
cargo test 2>&1
```

Expected: all `kitty::tests` and `protocol::tests` and `video::tests` pass. No failures.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: delete window.rs, renderer.rs, geometry.rs, xvfb-test.py"
```

---

### Task 6: Update lua/nvim-gfx/geometry.lua

**Files:**
- Modify: `lua/nvim-gfx/geometry.lua`

- [ ] **Step 1: Replace geometry.lua**

```lua
local M = {}

--- Return cell-space geometry for the given window handle.
--- row/col: 0-indexed terminal cell position of the window's top-left corner.
--- width/height: window dimensions in terminal cells.
function M.win_geometry(win)
    win = win or 0
    local pos = vim.api.nvim_win_get_position(win)
    return {
        row    = pos[1],
        col    = pos[2],
        width  = vim.api.nvim_win_get_width(win),
        height = vim.api.nvim_win_get_height(win),
    }
end

return M
```

- [ ] **Step 2: Commit**

```bash
git add lua/nvim-gfx/geometry.lua
git commit -m "refactor: geometry.lua — cell coords only, drop pixel math"
```

---

### Task 7: Update lua/nvim-gfx/viewer.lua

**Files:**
- Modify: `lua/nvim-gfx/viewer.lua`

- [ ] **Step 1: Update the show command calls**

In `M.open()`, find this block:

```lua
local geo = geometry.win_geometry(state.winid)
send({ cmd = "show", path = path,
       x = geo.x, y = geo.y, w = geo.w, h = geo.h,
       cols = geo.cols, rows = geo.rows })
```

Replace with:

```lua
local geo = geometry.win_geometry(state.winid)
send({ cmd = "show", path = path,
       row = geo.row, col = geo.col,
       width = geo.width, height = geo.height })
```

- [ ] **Step 2: Update on_resize()**

Find:

```lua
local function on_resize()
    if not state.job_id then return end
    local g = geometry.win_geometry(state.winid)
    send({ cmd = "show", path = state.path,
           x = g.x, y = g.y, w = g.w, h = g.h,
           cols = g.cols, rows = g.rows })
end
```

Replace with:

```lua
local function on_resize()
    if not state.job_id then return end
    local g = geometry.win_geometry(state.winid)
    send({ cmd = "show", path = state.path,
           row = g.row, col = g.col,
           width = g.width, height = g.height })
end
```

- [ ] **Step 3: Verify no other references to old geometry fields**

```bash
grep -n "geo\.x\|geo\.y\|geo\.w\b\|geo\.h\b\|geo\.cols\|geo\.rows\|g\.x\|g\.y\|g\.w\b\|g\.h\b\|g\.cols\|g\.rows" lua/nvim-gfx/viewer.lua
```

Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add lua/nvim-gfx/viewer.lua
git commit -m "refactor: viewer.lua — send cell coords in show command"
```

---

### Task 8: Build, Update test.sh, Smoke Test

**Files:**
- Modify: `scripts/test.sh`

- [ ] **Step 1: Build release binary and copy**

```bash
cargo build --release && cp target/release/nvim-gfx bin/nvim-gfx
```

Expected: `Finished release profile`, binary copied to `bin/nvim-gfx`.

- [ ] **Step 2: Replace scripts/test.sh**

```bash
#!/bin/bash
# Usage: ./scripts/test.sh [image_path] [video_path]
# Must be run from a real terminal (writes Kitty protocol to /dev/tty).
set -e

BINARY="./bin/nvim-gfx"
IMAGE="${1:-./53dd0a9a-5dad-4f78-a794-b630c22750b3.png}"

if [ ! -f "$BINARY" ]; then
    echo "Binary not found. Run: cargo build --release && cp target/release/nvim-gfx bin/nvim-gfx"
    exit 1
fi

if [ ! -f "$IMAGE" ]; then
    echo "Test image not found: $IMAGE"
    exit 1
fi

echo "Test 1: image open emits ready"
RESULT=$(printf '{"cmd":"show","path":"%s","row":0,"col":0,"width":80,"height":24}\n{"cmd":"quit"}\n' "$IMAGE" | "$BINARY" 2>/dev/null)
if echo "$RESULT" | grep -q '"event":"ready"'; then
    echo "  PASS"
else
    echo "  FAIL: $RESULT"
    exit 1
fi

echo "Test 2: bad path emits error"
RESULT=$(printf '{"cmd":"show","path":"/nonexistent.png","row":0,"col":0,"width":80,"height":24}\n' | timeout 3 "$BINARY" 2>/dev/null || true)
if echo "$RESULT" | grep -q '"event":"error"'; then
    echo "  PASS"
else
    echo "  FAIL: $RESULT"
    exit 1
fi

echo "All tests passed."
```

```bash
chmod +x scripts/test.sh
```

- [ ] **Step 3: Run smoke test**

```bash
./scripts/test.sh
```

Expected:
```
Test 1: image open emits ready
  PASS
Test 2: bad path emits error
  PASS
All tests passed.
```

Note: Test 1 will briefly write Kitty protocol data to your terminal (image flashes then disappears when quit is sent). This is expected.

- [ ] **Step 4: Commit**

```bash
git add scripts/test.sh bin/nvim-gfx
git commit -m "chore: update test.sh for Kitty protocol binary, copy release binary"
```

---

### Task 9: End-to-End Human Test (Ghostty)

This task requires a live Ghostty terminal session with Neovim.

- [ ] **Step 1: Open an image in Neovim**

```
:ViewImage /path/to/image.png
```

Expected:
- Image fills the current Neovim window
- Sidebar (if open) remains visible; image fills only the editor pane
- `q` closes the viewer, returns to previous buffer

- [ ] **Step 2: Test zoom and pan**

With the image open:
- `+` zooms in, `-` zooms out
- `h`/`l`/`j`/`k` pan the image
- `r` resets zoom and pan to default
- `q` closes

- [ ] **Step 3: Open a video file**

```
:ViewImage /path/to/video.mp4
```

Expected:
- Video plays automatically
- `p` or `<Space>` pauses; same key resumes
- `h`/`<Left>` seeks back 1s; `l`/`<Right>` seeks forward 1s
- `j`/`<Down>` seeks back 10s; `k`/`<Up>` seeks forward 10s
- `r` rewinds to start and resumes
- `+`/`=` zoom in; `-` zoom out
- Video pauses on last frame (no loop)
- `q` closes cleanly

- [ ] **Step 4: Verify resize**

With an image open: resize the terminal or toggle the sidebar. Image should reflow to fill the new window dimensions.

- [ ] **Step 5: Commit**

```bash
git commit --allow-empty -m "chore: Kitty protocol rewrite e2e verified"
git push
```
