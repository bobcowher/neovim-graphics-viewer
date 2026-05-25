# nvim-gfx Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Neovim plugin that displays images (PNG/JPEG/WebP) as a borderless X11/XWayland overlay window positioned over the current buffer area, controlled via Neovim keymaps.

**Architecture:** A Rust binary handles window creation (winit + softbuffer) and image decoding, receiving commands over stdin (newline-delimited JSON) and emitting events to stdout. The Lua plugin manages job lifecycle, buffer-local keymaps, and geometry computation from Neovim's window API. All user input flows through Neovim — the overlay never steals focus.

**Tech Stack:** Rust (winit 0.30, softbuffer 0.4, image 0.25, x11rb 0.13, libc, serde_json 1), Lua (Neovim API — jobstart, nvim_win_get_position, autocommands, vim.json)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | Create | Rust package config and dependencies |
| `src/protocol.rs` | Create | Serde Command/Event enums for stdin/stdout IPC |
| `src/geometry.rs` | Create | Cell→pixel math, TIOCGWINSZ, X11 window position |
| `src/renderer.rs` | Create | Image decode, zoom/pan state, pixel buffer writes |
| `src/window.rs` | Create | winit App struct, softbuffer rendering, command dispatch |
| `src/main.rs` | Create | Entry point: stdin reader thread + winit event loop |
| `lua/nvim-gfx/geometry.lua` | Create | Window cell geometry from Neovim API |
| `lua/nvim-gfx/viewer.lua` | Create | Job lifecycle, stdin commands, stdout events, keymaps |
| `lua/nvim-gfx/init.lua` | Create | setup(), autocommands, :ViewImage, :NvimGfxBuild |
| `plugin/nvim-gfx.lua` | Create | Plugin manager entry point (load guard only) |
| `scripts/test.sh` | Create | Launch Neovim with plugin injected into rtp |
| `scripts/test-init.lua` | Create | Minimal init: real config + nvim-gfx setup() |
| `.gitignore` | Create | Ignore target/, bin/ |

---

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`

- [ ] **Step 1: Create .gitignore**

```
/target/
/bin/
.superpowers/
```

- [ ] **Step 2: Create Cargo.toml**

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
winit = "0.30"
softbuffer = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
x11rb = { version = "0.13", features = ["allow-unsafe-code"] }
libc = "0.2"
```

- [ ] **Step 3: Create src/ stub so cargo check works**

```bash
mkdir -p src lua/nvim-gfx plugin scripts bin
touch src/main.rs
echo 'fn main() {}' > src/main.rs
```

- [ ] **Step 4: Verify cargo check passes**

Run: `cargo check`
Expected: no errors (empty main compiles)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml .gitignore src/main.rs
git commit -m "chore: project scaffold — Cargo.toml, .gitignore, empty main"
```

---

### Task 2: Protocol Types

**Files:**
- Create: `src/protocol.rs`
- Modify: `src/main.rs` (add `mod protocol;`)

The protocol uses serde's `tag` attribute so `{"cmd":"show",...}` deserializes to `Command::Show{...}` without a wrapper key.

- [ ] **Step 1: Write the failing test**

In `src/protocol.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Show { path: String, x: u32, y: u32, w: u32, h: u32 },
    Zoom { factor: f32 },
    Pan { dx: i32, dy: i32 },
    Reset,
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
    fn deserialize_show() {
        let json = r#"{"cmd":"show","path":"/tmp/a.png","x":0,"y":0,"w":80,"h":24}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Show { path, x, y, w, h } => {
                assert_eq!(path, "/tmp/a.png");
                assert_eq!(x, 0);
                assert_eq!(w, 80);
                assert_eq!(h, 24);
            }
            _ => panic!("wrong variant"),
        }
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
}
```

- [ ] **Step 2: Add mod to main.rs**

Replace `src/main.rs` with:
```rust
mod protocol;
fn main() {}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test protocol`
Expected: 7 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/protocol.rs src/main.rs
git commit -m "feat: protocol serde types for stdin/stdout IPC"
```

---

### Task 3: Renderer

**Files:**
- Create: `src/renderer.rs`
- Modify: `src/main.rs` (add `mod renderer;`)

The renderer owns zoom/pan state and writes XRGB pixels to a `&mut [u32]` buffer. It has no dependency on winit or any display — fully unit-testable. Zoom state starts at 1.0 (fit-to-window). Pan is stored in pixels.

- [ ] **Step 1: Write the failing tests**

Create `src/renderer.rs` with tests first:

```rust
use image::DynamicImage;

pub struct Renderer {
    image: Option<DynamicImage>,
    pub zoom_level: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Renderer {
    pub fn new() -> Self {
        Self { image: None, zoom_level: 1.0, pan_x: 0.0, pan_y: 0.0 }
    }

    pub fn load(&mut self, path: &str) -> Result<(), String> {
        let img = image::open(path).map_err(|e| format!("failed to decode {path}: {e}"))?;
        self.image = Some(img);
        self.zoom_level = 1.0;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        Ok(())
    }

    pub fn zoom(&mut self, factor: f32) {
        self.zoom_level = (self.zoom_level * factor).max(0.05).min(32.0);
    }

    pub fn pan(&mut self, dx_px: f32, dy_px: f32) {
        self.pan_x += dx_px;
        self.pan_y += dy_px;
    }

    pub fn reset(&mut self) {
        self.zoom_level = 1.0;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }

    /// Write XRGB pixels to buffer. Format: 0x00RRGGBB per u32.
    /// Image is fit-to-window then zoom/pan applied. Centered when zoom_level==1.0.
    pub fn render(&self, buffer: &mut [u32], width: u32, height: u32) {
        let Some(ref img) = self.image else {
            buffer.iter_mut().for_each(|p| *p = 0x1a1a2e);
            return;
        };

        let rgba = img.to_rgba8();
        let (img_w, img_h) = rgba.dimensions();

        let fit_scale = (width as f32 / img_w as f32).min(height as f32 / img_h as f32);
        let effective_zoom = fit_scale * self.zoom_level;

        // Center offset for fit-to-window base, then apply pan
        let center_x = (width as f32 - img_w as f32 * effective_zoom) / 2.0 + self.pan_x;
        let center_y = (height as f32 - img_h as f32 * effective_zoom) / 2.0 + self.pan_y;

        for py in 0..height {
            for px in 0..width {
                let sx = ((px as f32 - center_x) / effective_zoom) as i32;
                let sy = ((py as f32 - center_y) / effective_zoom) as i32;

                let pixel = if sx >= 0 && sx < img_w as i32 && sy >= 0 && sy < img_h as i32 {
                    let p = rgba.get_pixel(sx as u32, sy as u32).0;
                    ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
                } else {
                    0x1a1a2e
                };

                buffer[(py * width + px) as usize] = pixel;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let r = Renderer::new();
        assert!((r.zoom_level - 1.0).abs() < 0.001);
        assert_eq!(r.pan_x, 0.0);
        assert_eq!(r.pan_y, 0.0);
    }

    #[test]
    fn zoom_multiplies() {
        let mut r = Renderer::new();
        r.zoom(2.0);
        r.zoom(1.5);
        assert!((r.zoom_level - 3.0).abs() < 0.001);
    }

    #[test]
    fn zoom_clamped_at_min() {
        let mut r = Renderer::new();
        r.zoom(0.0001);
        assert!(r.zoom_level >= 0.05);
    }

    #[test]
    fn zoom_clamped_at_max() {
        let mut r = Renderer::new();
        r.zoom(9999.0);
        assert!(r.zoom_level <= 32.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut r = Renderer::new();
        r.zoom(3.0);
        r.pan(100.0, 50.0);
        r.reset();
        assert!((r.zoom_level - 1.0).abs() < 0.001);
        assert_eq!(r.pan_x, 0.0);
        assert_eq!(r.pan_y, 0.0);
    }

    #[test]
    fn pan_accumulates() {
        let mut r = Renderer::new();
        r.pan(30.0, 16.0);
        r.pan(-10.0, 8.0);
        assert!((r.pan_x - 20.0).abs() < 0.001);
        assert!((r.pan_y - 24.0).abs() < 0.001);
    }

    #[test]
    fn render_empty_fills_background() {
        let r = Renderer::new();
        let mut buf = vec![0u32; 4];
        r.render(&mut buf, 2, 2);
        assert!(buf.iter().all(|&p| p == 0x1a1a2e));
    }

    #[test]
    fn load_bad_path_returns_err() {
        let mut r = Renderer::new();
        assert!(r.load("/nonexistent/path/image.png").is_err());
    }
}
```

- [ ] **Step 2: Add mod to main.rs**

```rust
mod protocol;
mod renderer;
fn main() {}
```

- [ ] **Step 3: Run tests**

Run: `cargo test renderer`
Expected: 8 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/renderer.rs src/main.rs
git commit -m "feat: renderer — image decode, zoom/pan state, pixel buffer writes"
```

---

### Task 4: Geometry Module

**Files:**
- Create: `src/geometry.rs`
- Modify: `src/main.rs` (add `mod geometry;`)

Converts cell coordinates (from Neovim) to screen pixel coordinates. Uses `TIOCGWINSZ` for cell pixel size and X11 (`x11rb`) for terminal window screen position.

- [ ] **Step 1: Write the failing tests**

Create `src/geometry.rs`:

```rust
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

#[derive(Debug, Clone)]
pub struct PixelGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Convert cell-space geometry to screen pixel geometry.
pub fn compute_geometry(x_cells: u32, y_cells: u32, w_cells: u32, h_cells: u32) -> Result<PixelGeometry, String> {
    let (cell_w, cell_h) = cell_pixel_size()?;
    let (term_x, term_y) = terminal_screen_position()?;
    Ok(PixelGeometry {
        x: term_x + (x_cells * cell_w) as i32,
        y: term_y + (y_cells * cell_h) as i32,
        width: (w_cells * cell_w).max(1),
        height: (h_cells * cell_h).max(1),
    })
}

/// Return (cell_pixel_width, cell_pixel_height) from the terminal.
pub fn cell_pixel_size() -> Result<(u32, u32), String> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        let ret = libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws);
        if ret != 0 {
            return Err("ioctl TIOCGWINSZ failed".into());
        }
        if ws.ws_col == 0 || ws.ws_row == 0 {
            return Err("terminal reports zero cell dimensions".into());
        }
        if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 {
            return Err("terminal does not report pixel dimensions (try a different terminal)".into());
        }
        Ok((
            ws.ws_xpixel as u32 / ws.ws_col as u32,
            ws.ws_ypixel as u32 / ws.ws_row as u32,
        ))
    }
}

/// Return (x, y) screen position of the terminal window via X11.
pub fn terminal_screen_position() -> Result<(i32, i32), String> {
    let (conn, screen_num) = x11rb::connect(None)
        .map_err(|e| format!("X11 connect failed: {e}"))?;
    let root = conn.setup().roots[screen_num].root;
    let win_id = terminal_window_id(&conn, root)?;

    let reply = conn
        .translate_coordinates(win_id, root, 0, 0)
        .map_err(|e| format!("translate_coordinates request failed: {e}"))?
        .reply()
        .map_err(|e| format!("translate_coordinates reply failed: {e}"))?;

    Ok((reply.dst_x as i32, reply.dst_y as i32))
}

fn terminal_window_id(conn: &impl Connection + x11rb::protocol::xproto::ConnectionExt, root: u32) -> Result<u32, String> {
    // Prefer $WINDOWID (set by most terminal emulators)
    if let Ok(s) = std::env::var("WINDOWID") {
        if let Ok(id) = s.parse::<u32>() {
            if id != 0 {
                return Ok(id);
            }
        }
    }

    // Fallback: _NET_ACTIVE_WINDOW from the root window
    let atom_reply = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|e| format!("intern_atom failed: {e}"))?
        .reply()
        .map_err(|e| format!("intern_atom reply failed: {e}"))?;

    let prop = conn
        .get_property(false, root, atom_reply.atom, AtomEnum::WINDOW, 0, 1)
        .map_err(|e| format!("get_property failed: {e}"))?
        .reply()
        .map_err(|e| format!("get_property reply failed: {e}"))?;

    if prop.value.len() >= 4 {
        let id = u32::from_ne_bytes(prop.value[..4].try_into().unwrap());
        if id != 0 {
            return Ok(id);
        }
    }

    Err("cannot determine terminal window ID — set $WINDOWID or use a compatible terminal".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for the pure arithmetic — no ioctl or X11 needed.

    fn make_geo(x_cells: u32, y_cells: u32, w_cells: u32, h_cells: u32,
                term_x: i32, term_y: i32, cell_w: u32, cell_h: u32) -> PixelGeometry {
        PixelGeometry {
            x: term_x + (x_cells * cell_w) as i32,
            y: term_y + (y_cells * cell_h) as i32,
            width: (w_cells * cell_w).max(1),
            height: (h_cells * cell_h).max(1),
        }
    }

    #[test]
    fn geometry_math_origin() {
        let g = make_geo(0, 0, 80, 24, 100, 200, 8, 16);
        assert_eq!(g.x, 100);
        assert_eq!(g.y, 200);
        assert_eq!(g.width, 640);
        assert_eq!(g.height, 384);
    }

    #[test]
    fn geometry_math_offset() {
        let g = make_geo(5, 3, 70, 20, 0, 0, 10, 20);
        assert_eq!(g.x, 50);
        assert_eq!(g.y, 60);
        assert_eq!(g.width, 700);
        assert_eq!(g.height, 400);
    }

    #[test]
    fn geometry_zero_cells_gives_min_one() {
        let g = make_geo(0, 0, 0, 0, 0, 0, 8, 16);
        assert_eq!(g.width, 1);
        assert_eq!(g.height, 1);
    }
}
```

- [ ] **Step 2: Add mod to main.rs**

```rust
mod geometry;
mod protocol;
mod renderer;
fn main() {}
```

- [ ] **Step 3: Run tests**

Run: `cargo test geometry`
Expected: 3 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/geometry.rs src/main.rs
git commit -m "feat: geometry module — TIOCGWINSZ + X11 cell-to-pixel conversion"
```

---

### Task 5: Window Management

**Files:**
- Create: `src/window.rs`
- Modify: `src/main.rs` (add `mod window;`)

The `App` struct implements winit's `ApplicationHandler<Command>`. The winit event loop runs on the main thread; commands arrive as `UserEvent`s sent from the stdin reader thread via `EventLoopProxy`. The window is created on first `Show` command, hidden until the first render completes.

No unit tests — requires a live display. Verified in Task 7.

- [ ] **Step 1: Create src/window.rs**

```rust
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
                Err(e) => emit(Event::Error { msg: format!("window creation failed: {e}") }),
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
            Command::Show { path, x, y, w, h } => {
                match geometry::compute_geometry(x, y, w, h) {
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
```

- [ ] **Step 2: Add mod to main.rs**

```rust
mod geometry;
mod protocol;
mod renderer;
mod window;
fn main() {}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/window.rs src/main.rs
git commit -m "feat: window management — winit App, softbuffer rendering, command dispatch"
```

---

### Task 6: Main Entry Point

**Files:**
- Modify: `src/main.rs`

The stdin reader runs on a background thread. It parses each line as a `Command` and sends it to the winit event loop via `EventLoopProxy`. Unknown/malformed lines are silently skipped. The event loop blocks the main thread.

- [ ] **Step 1: Write src/main.rs**

```rust
mod geometry;
mod protocol;
mod renderer;
mod window;

use std::io::BufRead;
use window::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();

    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() { continue; }
            match serde_json::from_str(&line) {
                Ok(cmd) => {
                    if proxy.send_event(cmd).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("nvim-gfx: bad command: {e}: {line}");
                }
            }
        }
    });

    let mut app = App::new();
    event_loop.run_app(&mut app).expect("event loop run");
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: compiles without errors (may have warnings about unused imports — those are fine)

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: main entry point — stdin reader thread + winit event loop"
```

---

### Task 7: Build Binary and Smoke Test

**Files:**
- Create: `bin/` directory (not committed)

- [ ] **Step 1: Build release binary**

Run: `cargo build --release`
Expected: `target/release/nvim-gfx` exists

- [ ] **Step 2: Copy to bin/**

Run: `mkdir -p bin && cp target/release/nvim-gfx bin/nvim-gfx`

- [ ] **Step 3: Smoke-test the quit command**

Run:
```bash
echo '{"cmd":"quit"}' | ./bin/nvim-gfx
echo "exit: $?"
```
Expected: process exits with code 0 (no display needed — quit is handled before window creation)

- [ ] **Step 4: Smoke-test bad input is tolerated**

Run:
```bash
printf 'not json\n{"cmd":"quit"}\n' | ./bin/nvim-gfx
```
Expected: exits cleanly; bad line logged to stderr, quit processed

- [ ] **Step 5: Commit build artifacts note**

`bin/` is in .gitignore — nothing to commit. Confirm:
```bash
git status
```
Expected: clean (bin/ not tracked)

---

### Task 8: Lua Geometry Module

**Files:**
- Create: `lua/nvim-gfx/geometry.lua`

Returns cell-space position and size of a Neovim window. This is the Lua counterpart to `src/geometry.rs` — it produces the x/y/w/h values sent in the `show` command.

- [ ] **Step 1: Create lua/nvim-gfx/geometry.lua**

```lua
local M = {}

--- Return {x, y, w, h} in terminal cells for the given window handle.
--- x = leftmost column, y = topmost row (0-indexed from terminal top-left).
--- w = width in columns, h = height in rows.
function M.win_geometry(win)
    win = win or 0
    local pos = vim.api.nvim_win_get_position(win)
    local w = vim.api.nvim_win_get_width(win)
    local h = vim.api.nvim_win_get_height(win)
    return {
        x = pos[2],
        y = pos[1],
        w = w,
        h = h,
    }
end

return M
```

- [ ] **Step 2: Verify it loads without error**

Run: `nvim --headless -u NONE -c "lua require('nvim-gfx.geometry')" -c "q"`
Expected: exits cleanly with no error output (add `lua/` to rtp first):

```bash
nvim --headless -u NONE \
  -c "lua vim.opt.rtp:prepend('$(pwd)')" \
  -c "lua require('nvim-gfx.geometry')" \
  -c "q" 2>&1
```
Expected: no output (no errors)

- [ ] **Step 3: Commit**

```bash
git add lua/nvim-gfx/geometry.lua
git commit -m "feat: lua geometry module — cell position from nvim window API"
```

---

### Task 9: Lua Viewer Module

**Files:**
- Create: `lua/nvim-gfx/viewer.lua`

Manages one active viewer session: spawns the Rust binary, sends commands, handles events, registers buffer-local keymaps, and handles resize. Only one viewer can be open at a time — opening a second closes the first.

- [ ] **Step 1: Create lua/nvim-gfx/viewer.lua**

```lua
local geometry = require("nvim-gfx.geometry")

local M = {}

local state = {
    job_id = nil,
    bufnr  = nil,
    winid  = nil,
    aug_id = nil,
    path   = nil,
}

local function binary_path()
    local src = debug.getinfo(1, "S").source:sub(2)
    local root = vim.fn.fnamemodify(src, ":h:h:h")
    return root .. "/bin/nvim-gfx"
end

local function send(cmd)
    if state.job_id then
        vim.fn.chansend(state.job_id, vim.json.encode(cmd) .. "\n")
    end
end

local function cleanup()
    if state.aug_id then
        pcall(vim.api.nvim_del_augroup_by_id, state.aug_id)
        state.aug_id = nil
    end
    if state.bufnr and vim.api.nvim_buf_is_valid(state.bufnr) then
        vim.api.nvim_buf_delete(state.bufnr, { force = true })
    end
    state.job_id = nil
    state.bufnr  = nil
    state.winid  = nil
    state.path   = nil
end

local function on_stdout(_, data, _)
    for _, line in ipairs(data) do
        if line ~= "" then
            local ok, ev = pcall(vim.json.decode, line)
            if ok and type(ev) == "table" and ev.event == "error" then
                vim.notify("nvim-gfx: " .. (ev.msg or "unknown error"), vim.log.levels.ERROR)
                M.close()
            end
        end
    end
end

local function on_exit(_, code, _)
    if code ~= 0 then
        vim.notify("nvim-gfx: binary exited with code " .. code, vim.log.levels.WARN)
    end
    cleanup()
end

local function set_keymaps(bufnr)
    local o = { noremap = true, silent = true, buffer = bufnr }
    vim.keymap.set("n", "q",       function() M.close() end, o)
    vim.keymap.set("n", "<Esc>",   function() M.close() end, o)
    vim.keymap.set("n", "+",       function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "=",       function() send({ cmd = "zoom", factor = 1.25 }) end, o)
    vim.keymap.set("n", "-",       function() send({ cmd = "zoom", factor = 0.8  }) end, o)
    vim.keymap.set("n", "r",       function() send({ cmd = "reset" }) end, o)
    vim.keymap.set("n", "h",       function() send({ cmd = "pan", dx = -1, dy =  0 }) end, o)
    vim.keymap.set("n", "<Left>",  function() send({ cmd = "pan", dx = -1, dy =  0 }) end, o)
    vim.keymap.set("n", "l",       function() send({ cmd = "pan", dx =  1, dy =  0 }) end, o)
    vim.keymap.set("n", "<Right>", function() send({ cmd = "pan", dx =  1, dy =  0 }) end, o)
    vim.keymap.set("n", "k",       function() send({ cmd = "pan", dx =  0, dy = -1 }) end, o)
    vim.keymap.set("n", "<Up>",    function() send({ cmd = "pan", dx =  0, dy = -1 }) end, o)
    vim.keymap.set("n", "j",       function() send({ cmd = "pan", dx =  0, dy =  1 }) end, o)
    vim.keymap.set("n", "<Down>",  function() send({ cmd = "pan", dx =  0, dy =  1 }) end, o)
end

function M.open(path)
    path = vim.fn.expand(path)
    if vim.fn.filereadable(path) == 0 then
        vim.notify("nvim-gfx: file not readable: " .. path, vim.log.levels.ERROR)
        return
    end

    local bin = binary_path()
    if vim.fn.executable(bin) == 0 then
        vim.notify("nvim-gfx: binary not found, run :NvimGfxBuild", vim.log.levels.ERROR)
        return
    end

    if state.job_id then M.close() end

    state.winid = vim.api.nvim_get_current_win()
    state.path  = path

    local bufnr = vim.api.nvim_create_buf(false, true)
    state.bufnr = bufnr
    vim.api.nvim_win_set_buf(state.winid, bufnr)
    vim.bo[bufnr].bufhidden = "wipe"

    set_keymaps(bufnr)

    state.job_id = vim.fn.jobstart({ bin }, {
        on_stdout = on_stdout,
        on_exit   = on_exit,
        stdout_buffered = false,
    })

    local geo = geometry.win_geometry(state.winid)
    send({ cmd = "show", path = path, x = geo.x, y = geo.y, w = geo.w, h = geo.h })

    state.aug_id = vim.api.nvim_create_augroup("NvimGfxResize" .. bufnr, { clear = true })
    vim.api.nvim_create_autocmd("VimResized", {
        group    = state.aug_id,
        callback = function()
            if not state.job_id then return end
            local g = geometry.win_geometry(state.winid)
            send({ cmd = "show", path = state.path, x = g.x, y = g.y, w = g.w, h = g.h })
        end,
    })
end

function M.close()
    if state.job_id then
        send({ cmd = "quit" })
        vim.fn.jobstop(state.job_id)
    end
    cleanup()
end

return M
```

- [ ] **Step 2: Verify it loads**

```bash
nvim --headless -u NONE \
  -c "lua vim.opt.rtp:prepend('$(pwd)')" \
  -c "lua require('nvim-gfx.viewer')" \
  -c "q" 2>&1
```
Expected: no output

- [ ] **Step 3: Commit**

```bash
git add lua/nvim-gfx/viewer.lua
git commit -m "feat: lua viewer — job lifecycle, IPC commands, keymaps, resize handling"
```

---

### Task 10: Lua Init + Plugin Entry Point

**Files:**
- Create: `lua/nvim-gfx/init.lua`
- Create: `plugin/nvim-gfx.lua`

`init.lua` exposes `setup()` which registers autocommands and user commands. `plugin/nvim-gfx.lua` is the plugin manager entry point — it only sets a load guard, never calls `setup()`. Users must call `setup()` explicitly.

- [ ] **Step 1: Create lua/nvim-gfx/init.lua**

```lua
local M = {}

local function plugin_root()
    local src = debug.getinfo(1, "S").source:sub(2)
    return vim.fn.fnamemodify(src, ":h:h:h")
end

function M.setup(_opts)
    local viewer = require("nvim-gfx.viewer")

    vim.api.nvim_create_user_command("ViewImage", function(a)
        viewer.open(a.args)
    end, { nargs = 1, complete = "file", desc = "View an image file" })

    vim.api.nvim_create_user_command("NvimGfxBuild", function()
        local root = plugin_root()
        local cmd = string.format(
            "cd %s && cargo build --release && mkdir -p bin && cp target/release/nvim-gfx bin/nvim-gfx",
            vim.fn.shellescape(root)
        )
        vim.notify("nvim-gfx: building (check :messages for result)...", vim.log.levels.INFO)
        vim.fn.jobstart({ "sh", "-c", cmd }, {
            on_exit = function(_, code)
                if code == 0 then
                    vim.notify("nvim-gfx: build complete — bin/nvim-gfx ready", vim.log.levels.INFO)
                else
                    vim.notify("nvim-gfx: build failed (code " .. code .. ")", vim.log.levels.ERROR)
                end
            end,
        })
    end, { desc = "Build nvim-gfx Rust binary" })

    vim.api.nvim_create_autocmd("BufReadPost", {
        pattern  = { "*.png", "*.jpg", "*.jpeg", "*.webp" },
        callback = function(ev) viewer.open(ev.file) end,
        desc     = "Auto-preview image files with nvim-gfx",
    })
end

return M
```

- [ ] **Step 2: Create plugin/nvim-gfx.lua**

```lua
if vim.g.loaded_nvim_gfx then return end
vim.g.loaded_nvim_gfx = true
```

- [ ] **Step 3: Verify both load cleanly**

```bash
nvim --headless -u NONE \
  -c "lua vim.opt.rtp:prepend('$(pwd)')" \
  -c "luafile plugin/nvim-gfx.lua" \
  -c "lua require('nvim-gfx').setup()" \
  -c "q" 2>&1
```
Expected: no output

- [ ] **Step 4: Commit**

```bash
git add lua/nvim-gfx/init.lua plugin/nvim-gfx.lua
git commit -m "feat: lua init — setup(), :ViewImage, :NvimGfxBuild, auto-preview"
```

---

### Task 11: Test Scripts

**Files:**
- Create: `scripts/test.sh`
- Create: `scripts/test-init.lua`

These let you test the plugin with your full real Neovim config (lazy.nvim, colorscheme, etc.) without touching `~/.config/nvim/init.lua`.

- [ ] **Step 1: Create scripts/test.sh**

```bash
#!/usr/bin/env bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec nvim -u "$DIR/scripts/test-init.lua" "$@"
```

- [ ] **Step 2: Create scripts/test-init.lua**

```lua
-- Injects nvim-gfx into the runtime path, then sources your real config.
-- nvim -u replaces init.lua selection; dofile sources it explicitly.
local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h")
vim.opt.rtp:prepend(plugin_dir)
dofile(vim.fn.expand("~/.config/nvim/init.lua"))
require("nvim-gfx").setup()
```

- [ ] **Step 3: Make test.sh executable**

Run: `chmod +x scripts/test.sh`

- [ ] **Step 4: Verify test.sh launches Neovim without errors**

Run: `./scripts/test.sh --headless -c "lua print('nvim-gfx ok')" -c "q" 2>&1`
Expected: prints `nvim-gfx ok`, exits cleanly

- [ ] **Step 5: Commit**

```bash
git add scripts/test.sh scripts/test-init.lua
git commit -m "feat: test scripts — isolated nvim launch with real config + nvim-gfx injected"
```

---

### Task 12: End-to-End Test

No files created. Verifies the full stack: Lua plugin → Rust binary → X11 overlay.

Requires: a running X11 or XWayland session, `bin/nvim-gfx` built (Task 7).

- [ ] **Step 1: Get a test image**

Run:
```bash
TEST_IMG=$(find /usr/share/pixmaps /usr/share/icons -name "*.png" 2>/dev/null | head -1)
echo "Using: $TEST_IMG"
```
Expected: prints a path to a PNG file. If empty, use any `.png`, `.jpg`, or `.webp` file you have on disk.

Set `TEST_IMG=/path/to/your/image.png` and use it in subsequent steps.

- [ ] **Step 2: Launch with the test script**

Run: `./scripts/test.sh /tmp/test.jpg`

Expected:
- Neovim opens with your full config (tokyonight theme, nvim-tree, etc.)
- The image is displayed immediately as an overlay over the buffer area
- No error notifications

- [ ] **Step 3: Test zoom controls**

With the image visible:
- Press `+` → image zooms in
- Press `-` → image zooms out
- Press `r` → image fits back to window

- [ ] **Step 4: Test pan controls**

Zoom in first (`+`), then:
- Press `h` / `l` / `j` / `k` → image pans in each direction

- [ ] **Step 5: Test close**

Press `q` → overlay disappears, Neovim returns to normal buffer

- [ ] **Step 6: Test :ViewImage command**

In Neovim: `:ViewImage /tmp/test.jpg`
Expected: image appears over current buffer

- [ ] **Step 7: Test auto-preview**

Run: `./scripts/test.sh /tmp/test.jpg` (passing the image as an argument)
Expected: image auto-previews when the buffer loads (BufReadPost trigger)

- [ ] **Step 8: Test resize**

With image visible, resize the terminal window.
Expected: overlay repositions to stay aligned with the buffer area

- [ ] **Step 9: Final commit**

```bash
git add .
git status  # confirm only expected files
git commit -m "chore: verify end-to-end test complete"
```
