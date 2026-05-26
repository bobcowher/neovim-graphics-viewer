# nvim-gfx Kitty Graphics Protocol Rewrite Design

## Goal

Replace the X11 overlay rendering backend with the Kitty graphics protocol. Drop winit, softbuffer, and all X11/WM interaction. The Rust binary writes Kitty escape sequences directly to `/dev/tty`; the terminal renders the image at the specified cell position. Lua plugin changes are minimal.

## Background

The previous approach used `override_redirect=true` X11 windows positioned over the Neovim terminal. This fought the window manager at every turn: positioning drift, fullscreen takeover, focus loops. These are inherent limitations of bypassing the WM, not fixable bugs.

The Kitty graphics protocol is the right abstraction: Neovim writes to the terminal, the terminal owns rendering. No separate window, no WM interaction, no focus issues.

## Target Environment

- Terminal: Ghostty (first-class Kitty graphics protocol support)
- Platform: Linux (Ubuntu 24.04)
- Neovim: standard splits/sidebar layout respected

## Architecture

```
Neovim (Lua plugin)
  │  stdin JSON commands
  ▼
Rust binary (nvim-gfx)
  │  ffmpeg decode / image load
  │  zoom/pan crop
  │  Kitty protocol encoding
  ▼
/dev/tty  →  Ghostty terminal  →  screen
```

The Lua plugin spawns the binary, sends JSON commands via stdin, reads JSON events from stdout. The binary opens `/dev/tty` independently and writes Kitty escape sequences directly — this path bypasses Neovim entirely and goes straight to the terminal.

## IPC Protocol

### Commands (Lua → Rust via stdin)

All coordinates and dimensions are in terminal cell units.

```json
{ "cmd": "show",       "path": "/path/to/file.png", "row": 0, "col": 0, "width": 80, "height": 40 }
{ "cmd": "zoom",       "factor": 1.25 }
{ "cmd": "pan",        "dx": -1, "dy": 0 }
{ "cmd": "reset" }
{ "cmd": "play_pause" }
{ "cmd": "seek",       "delta": 5 }
{ "cmd": "rewind" }
{ "cmd": "quit" }
```

`Hide` and `Unhide` are removed. They existed only to work around the X11 focus-stealing problem, which does not apply to Kitty placements.

### Events (Rust → Lua via stdout)

Unchanged:
```json
{ "event": "ready" }
{ "event": "error", "msg": "..." }
```

## Rust Binary

### Files

| File | Status | Notes |
|------|--------|-------|
| `src/main.rs` | Rewrite | No winit; stdin thread + main state loop |
| `src/protocol.rs` | Update | New Show fields; remove Hide/Unhide |
| `src/kitty.rs` | New | Kitty protocol encoder + /dev/tty writer |
| `src/video.rs` | Keep | No changes needed |
| `src/window.rs` | Delete | Entire X11/winit layer gone |
| `src/renderer.rs` | Delete | Softbuffer renderer gone |
| `src/geometry.rs` | Delete | Pixel geometry gone |
| `Cargo.toml` | Update | Remove winit, softbuffer |

### main.rs — Event Loop

No event loop framework. A dedicated thread reads stdin line-by-line, deserializes commands, and sends them via `mpsc::channel`. The main thread runs a state machine:

```
during video playback:
  loop {
    drain command channel (non-blocking try_recv)
    if playing && !finished && now >= next_frame_time:
      decode frame → write kitty frame → update next_frame_time
    else:
      sleep(1ms)
  }

during image display or video paused:
  block on cmd_rx.recv()
```

### kitty.rs — Kitty Protocol Encoder

**View state:** tracks `zoom: f32` (default 1.0) and `pan_x, pan_y: f32` (default 0.0). These define a crop rectangle on the source pixels.

**Crop calculation:**
```
visible_w = src_width / zoom
visible_h = src_height / zoom
crop_x = clamp((src_width - visible_w) / 2 + pan_x, 0, src_width - visible_w)
crop_y = clamp((src_height - visible_h) / 2 + pan_y, 0, src_height - visible_h)
```

**Writing an image to the terminal:**
1. Write ANSI cursor position: `ESC[{row+1};{col+1}H`
2. Apply crop to produce a sub-region of the RGBA pixel buffer
3. Base64-encode the cropped RGBA data
4. Send Kitty sequence chunked at 4096 base64-char boundaries:
   - First/middle chunks: `ESC_Ga=T,f=32,s={w},v={h},c={cols},r={rows},i=1,m=1;{chunk}ESC\`
   - Last chunk: `ESC_Gm=0;{chunk}ESC\`
5. Write cursor hide: `ESC[?25l`

**Format details:**
- `a=T` — transmit and display
- `f=32` — raw RGBA (32-bit per pixel)
- `s,v` — pixel dimensions of the cropped source data
- `c,r` — target cell dimensions; Kitty scales to fit
- `i=1` — fixed image ID (one placement at a time)
- `q=2` — suppress Kitty's OK/error response

**On close / quit:** send delete sequence: `ESC_Ga=d,d=i,i=1ESC\`

**On resize / new show (same path):** re-crop and re-send with updated `c,r`.

### Cargo.toml Changes

Remove:
- `winit`
- `softbuffer`
- `x11rb`
- `libc`

Add:
- `base64` (for Kitty protocol payload encoding)

Keep:
- `ffmpeg-next`
- `image`
- `serde`, `serde_json`

## Lua Plugin

### geometry.lua

Simplified. No pixel math required.

```lua
function M.win_geometry(winid)
    local screenpos = vim.fn.win_screenpos(winid)
    return {
        row    = screenpos[1] - 1,   -- 0-indexed
        col    = screenpos[2] - 1,   -- 0-indexed
        width  = vim.api.nvim_win_get_width(winid),
        height = vim.api.nvim_win_get_height(winid),
    }
end
```

### viewer.lua

Changes:
- `show` command sends `row, col, width, height` instead of `x, y, w, h, cols, rows`
- Remove `FocusLost` / `FocusGained` autocmds
- Resize autocmd stays (re-sends `show` with updated geometry)
- All keymaps unchanged

## Display Behaviour

- Image fills the current Neovim window, respecting existing splits
- Sidebar open → image fills the editor pane only
- Sidebar closed → image fills the full terminal window
- Auto-scales to fit the window cell dimensions (Kitty handles scaling)
- On `q`: Rust sends Kitty delete sequence, Lua wipes the scratch buffer

## Video Playback

- Auto-plays on open
- `p` / `<Space>`: play/pause
- `h` / `<Left>`: seek −1s; `l` / `<Right>`: seek +1s
- `j` / `<Down>`: seek −10s; `k` / `<Up>`: seek +10s
- `r`: rewind to start
- `+` / `=`: zoom in; `-`: zoom out
- Pauses on last frame (no loop)
- Frame timing: main loop sleeps 1ms between iterations, renders when `now >= next_frame_time`

## Error Handling

- Binary emits `{"event":"error","msg":"..."}` on decode failure or tty write failure
- Lua displays via `vim.notify` and calls `M.close()`
- Unsupported file type: Lua rejects before spawning binary (same as today)

## Out of Scope

- Wayland-native rendering (XWayland covers this for now)
- GIF support
- Non-Ghostty terminals (sixel fallback, iTerm2 protocol)
- Inline-in-buffer rendering (image.nvim style with Unicode placeholders)
