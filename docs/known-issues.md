# Known Issues & Technical Requirements

Concrete problems with the current implementation. Each item describes the symptom,
the root cause in the code, and exactly what correct behavior looks like.

---

## Open

### 7. Paused State Always Displays "playing"

**Symptom:** After pressing pause, the buffer name still reads `playing 01:23 / 05:47`.
The playing/paused indicator never changes.

**Root cause:** The `Time` event carries `position` and `duration` but no play state.
In `on_stdout`, the Lua side hardcodes `local status = "playing"` regardless of what
the binary reports.

**Required behavior:**

- Add a `playing: bool` field to the `Time` event on the Rust side.
- The Rust binary sets it based on `vs.decoder.is_playing()` at the time of emission.
- The Lua side uses it to display either `playing` or `paused` in the buffer name.

---

### 8. Seek and Play/Pause Position Updates Are Throttled

**Symptom:** If the user seeks or pauses within 900ms of the last position update,
the buffer name does not update. The user seeks and sees stale position feedback.

**Root cause:** `emit_time` applies a 900ms gate to every call, including those
triggered by explicit user actions (seek, rewind, play/pause):

```rust
if now.duration_since(vs.last_time_emit) >= Duration::from_millis(900) {
```

The throttle was intended for the automatic per-frame tick, not for user interactions.

**Required behavior:**

- Seek, rewind, and play/pause must emit a `Time` event immediately, bypassing the
  throttle. The throttle should only apply during automatic frame advancement.
- The simplest fix: extract a `force_emit_time` path that skips the duration check,
  and call it from the seek/rewind/play-pause handlers. Keep the throttled path for
  the frame-tick call in the main render loop.

---

### 9. Unknown Video Duration Displays Garbage

**Symptom:** Some containers (certain MKV files, streams) report no duration.
`input_ctx.duration()` returns `AV_NOPTS_VALUE` (`i64::MIN`). Cast to `f64` and
divided by `AV_TIME_BASE`, this produces a very large negative number that renders
as nonsense in the buffer name.

**Root cause:** No guard on `input_ctx.duration()` before converting to seconds.

**Required behavior:**

- On open, check if `input_ctx.duration() <= 0`. If so, set `duration_secs = 0.0`
  as a sentinel.
- On the Lua side, if `ev.duration == 0`, display `--:--` instead of a formatted
  time for the duration field.

---

## Resolved

### 1. Image Disappears on Neovim Redraw ✓

Fixed: `Redraw` command added; Lua sends it on `CursorMoved`, `WinEnter`,
`BufEnter`, `ModeChanged`, `WinScrolled` with 100ms debounce via `vim.loop` timer.

### 2. Video Frame Timing Drifts ✓

Fixed: PTS-based timing via `stream_epoch`. Frame dropping loop added in main.
Epoch resets correctly on seek, rewind, and resume.

### 3. No Video Position Feedback ✓

Fixed: `Time { position, duration }` event emitted by binary; Lua updates buffer
name. (Partially — see issues 7, 8, 9 above.)

### 4. Redundant Pixel Format Conversion ✓

Fixed: ffmpeg scaler now outputs `Pixel::RGBA` directly. `to_rgba` copies raw bytes
per row (stride-aware). `xrgb_to_rgba` and `to_xrgb` removed from video path.

### 5. Hardcoded Image ID Collides with Other Kitty Clients ✓

Fixed: `KITTY_IMAGE_ID = 31337` constant; used in both display and clear.

### 6. TTY Resolution Is Linux-Only ✓

Fixed: Lua detects `vim.loop.os_uname().sysname == "Darwin"` and uses `/dev/fd/1`
on macOS, `/proc/self/fd/1` on Linux. Fails explicitly if path doesn't resolve.
