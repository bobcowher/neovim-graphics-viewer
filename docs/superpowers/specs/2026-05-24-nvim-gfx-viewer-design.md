# nvim-gfx: Neovim Image Viewer Plugin — Design Spec

**Date:** 2026-05-24  
**Scope:** v1 — image viewing only (PNG, JPEG, WebP). Video and GIF deferred to v2.  
**Target platform:** Ubuntu Linux (X11 and Wayland via XWayland)

---

## Overview

A lightweight Neovim plugin for viewing images. Two components: a Lua plugin (installed via any plugin manager) and a Rust binary (built locally with `cargo build --release`). The Rust binary renders an X11/Wayland overlay window positioned precisely over the current Neovim buffer area. All user input flows through Neovim keymaps — the overlay never steals focus.

---

## Architecture

```
Neovim (Lua plugin)
  ├─ autocommands: triggers on image filetypes (png, jpg, webp)
  ├─ :ViewImage <path> command
  ├─ viewer buffer: scratch buffer with local keymaps
  └─ jobstart() → Rust binary (stdin/stdout JSON)
                        │
                  Rust binary
                  ├─ stdin loop: receives commands
                  ├─ image decoder (image crate: PNG/JPEG/WebP)
                  ├─ X11/Wayland overlay window (winit + softbuffer)
                  └─ stdout: events back to Neovim
```

---

## Triggering

Two ways to open the viewer:

1. **Auto-preview:** Opening a file with extension `.png`, `.jpg`, `.jpeg`, or `.webp` automatically shows the viewer via a `BufReadPost` autocommand.
2. **Explicit command:** `:ViewImage <path>` takes a filename argument. Supports hidden files, paths not in any buffer, shell expansion via `expand()`.

Both paths converge on the same viewer launch flow.

---

## Viewer Launch Flow

1. Neovim opens a scratch buffer in the current window (for `:ViewImage`) or uses the image file's buffer (for auto-preview).
2. Buffer-local keymaps are set for viewer controls.
3. Lua computes the buffer window's position and size in terminal cells using `nvim_win_get_position()`, `nvim_win_get_width()`, `nvim_win_get_height()`.
4. Lua spawns the Rust binary via `jobstart()`, wiring stdin/stdout.
5. Lua sends the initial `show` command with path and cell geometry.
6. The binary renders the overlay. On success, emits `{"event":"ready"}`.
7. On `q` or `<Esc>`, Neovim sends `{"cmd":"quit"}`, the binary exits cleanly, and Neovim closes the scratch buffer.

---

## Display

The overlay covers the current buffer's screen area exactly — same position and size as the Neovim window. Works correctly alongside splits, file trees, and floating windows because it targets a specific window's cell region.

The overlay is a borderless window without decorations. On X11 and XWayland it is raised above the terminal via `_NET_WM_STATE_ABOVE`. On pure Wayland, always-on-top behavior is compositor-dependent and not guaranteed in v1 — XWayland is the supported path. The overlay does not receive keyboard focus — all input is handled by Neovim.

---

## IPC Protocol

Newline-delimited JSON over stdin/stdout.

### Commands (Neovim → Rust)

| Command | Fields | Description |
|---------|--------|-------------|
| `show` | `path`, `x`, `y`, `w`, `h` (cells) | Show image; geometry in terminal cells |
| `zoom` | `factor` (f32) | Multiply current zoom by factor |
| `pan` | `dx`, `dy` (i32, cells) | Pan by delta |
| `reset` | — | Reset zoom and pan to fit-to-window |
| `quit` | — | Exit cleanly |

`show` doubles as a reposition command — Lua re-sends it on `VimResized`.

### Events (Rust → Neovim)

| Event | Fields | Description |
|-------|--------|-------------|
| `ready` | — | Overlay is visible |
| `error` | `msg` | Decode or display failure |

The binary never dismisses itself. Only Neovim initiates close via `quit`.

### Geometry: Cell → Pixel Conversion

The Rust binary converts cell coordinates to screen pixels:
1. Read terminal pixel dimensions via `TIOCGWINSZ` (total pixel width/height and cell count).
2. Compute cell size: `cell_px = terminal_px / terminal_cells`.
3. Look up the terminal window's screen position via `$WINDOWID` + X11 (`x11rb`).
4. Fallback: query `_NET_ACTIVE_WINDOW` if `$WINDOWID` is unset.
5. If X11 position lookup fails entirely, emit `{"event":"error","msg":"cannot determine terminal position"}`.

---

## Keymaps (Buffer-local, set while viewer is active)

| Key | Action | Command sent |
|-----|--------|--------------|
| `q`, `<Esc>` | Close viewer | `quit` |
| `+`, `=` | Zoom in (1.25×) | `zoom {"factor":1.25}` |
| `-` | Zoom out (0.8×) | `zoom {"factor":0.8}` |
| `r` | Reset / fit to window | `reset` |
| `h`, `<Left>` | Pan left | `pan {"dx":-1,"dy":0}` |
| `l`, `<Right>` | Pan right | `pan {"dx":1,"dy":0}` |
| `k`, `<Up>` | Pan up | `pan {"dx":0,"dy":-1}` |
| `j`, `<Down>` | Pan down | `pan {"dx":0,"dy":1}` |

---

## Components

### Lua plugin (`lua/nvim-gfx/`)

| File | Responsibility |
|------|---------------|
| `init.lua` | `setup()`, registers autocommands and `:ViewImage` command |
| `viewer.lua` | Job lifecycle, stdin command dispatch, stdout event handling, keymap registration |
| `geometry.lua` | Cell-coordinate computation from Neovim window API |

### Rust binary (`src/`)

| File | Responsibility |
|------|---------------|
| `main.rs` | Entry point, stdin command loop, dispatches to modules |
| `protocol.rs` | `serde` types for commands and events |
| `window.rs` | `winit` window creation, borderless/always-on-top, geometry, repositioning |
| `renderer.rs` | Image decode (`image` crate), zoom/pan state, pixel buffer writes via `softbuffer` |

### Cargo dependencies

```toml
[dependencies]
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "webp"] }
winit = "0.30"
softbuffer = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
x11rb = "0.13"
```

---

## Project Layout

```
neovim-graphics-viewer/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── main.rs
│   ├── protocol.rs
│   ├── window.rs
│   └── renderer.rs
├── lua/
│   └── nvim-gfx/
│       ├── init.lua
│       ├── viewer.lua
│       └── geometry.lua
├── plugin/
│   └── nvim-gfx.lua        ← plugin manager entry point
├── bin/                    ← compiled binary lands here
└── docs/
    └── superpowers/specs/
```

---

## Installation (v1)

```bash
cd ~/.local/share/nvim/site/pack/plugins/start/neovim-graphics-viewer
cargo build --release
# or use :NvimGfxBuild from inside Neovim
```

`:NvimGfxBuild` is a convenience command that runs `cargo build --release` in the plugin directory and copies the binary to `bin/nvim-gfx`.

The Lua plugin locates the binary at `<plugin_root>/bin/nvim-gfx`, where `<plugin_root>` is derived from the path of `plugin/nvim-gfx.lua` via `vim.fn.fnamemodify(debug.getinfo(1).source:sub(2), ":h:h")`. If the binary is not found there, it is not searched on `$PATH` — the explicit path keeps behavior predictable.

---

## Error Handling

| Situation | Behavior |
|-----------|----------|
| Binary not built | One-time `vim.notify` error on first use: `"nvim-gfx: binary not found, run :NvimGfxBuild"` |
| Image decode failure | Rust emits `error` event; Lua notifies and closes scratch buffer |
| Binary crash / non-zero exit | `on_exit` callback notifies user; `jobstop()` always called on buffer close |
| `$WINDOWID` missing | Falls back to `_NET_ACTIVE_WINDOW`; fails with error event if X11 position is unresolvable |
| Neovim resize | `VimResized` autocommand re-sends `show` with updated geometry; binary repositions in-place |

---

## Test Script

To test the plugin without touching the user's real Neovim config:

```bash
# scripts/test.sh
#!/usr/bin/env bash
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
nvim -u "$DIR/scripts/test-init.lua" "$@"
```

```lua
-- scripts/test-init.lua
-- Loads the user's real config (~/.config/nvim/init.lua) with nvim-gfx
-- injected into rtp before lazy.nvim initializes.
local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1).source:sub(2), ":h:h")

-- Prepend before real config so plugin/ is in rtp when lazy processes it.
-- Lazy only manages its own plugin paths — manual prepends survive.
vim.opt.rtp:prepend(plugin_dir)

dofile(vim.fn.expand("~/.config/nvim/init.lua"))

-- Call setup() after lazy finishes. plugin/nvim-gfx.lua is auto-sourced
-- via rtp but does not call setup() itself — that stays explicit.
require("nvim-gfx").setup()
```

Usage:
```bash
./scripts/test.sh path/to/image.png
# or just
./scripts/test.sh
# then :ViewImage path/to/image.png
```

`nvim -u` replaces init file selection — `~/.config/nvim/init.lua` is sourced explicitly via `dofile`, so the full real config (lazy plugins, keymaps, colorscheme) loads normally. Pass extra args through to Neovim via `"$@"`.

---

## Out of Scope (v1)

- Video playback (v2)
- Animated GIF (v2)
- AVIF, SVG, TIFF, BMP
- Pre-built release binaries / CI
- Mouse input on the overlay
- Image metadata display (EXIF, dimensions)
