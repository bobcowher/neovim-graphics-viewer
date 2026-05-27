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
        self.pan_x += dx as f32 * 0.1;
        self.pan_y += dy as f32 * 0.1;
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
        if pixels.len() != (src_w * src_h * 4) as usize {
            return Err(format!(
                "pixel buffer mismatch: got {}, expected {}x{}x4={}",
                pixels.len(), src_w, src_h, src_w * src_h * 4
            ));
        }
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

fn open_tty() -> Result<Box<dyn Write>, String> {
    match OpenOptions::new().write(true).open("/dev/tty") {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => {
            eprintln!("nvim-gfx: cannot open /dev/tty ({e}), falling back to stdout");
            Ok(Box::new(std::io::stdout()))
        }
    }
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

    // Save cursor position, then move to image origin
    write!(tty, "\x1b7\x1b[{};{}H", row + 1, col + 1)
        .map_err(|e| format!("cursor position: {e}"))?;

    let b64 = STANDARD.encode(pixels);
    let b64_bytes = b64.as_bytes();
    let total_chunks = (b64_bytes.len() + 4095) / 4096;

    for (i, chunk) in b64_bytes.chunks(4096).enumerate() {
        let chunk_str = std::str::from_utf8(chunk).unwrap(); // SAFETY: base64 output is ASCII
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

    // Restore cursor position
    write!(tty, "\x1b8").map_err(|e| format!("cursor restore: {e}"))?;
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
    fn pan_large_zoom_can_reach_edge() {
        let mut r = KittyRenderer::new();
        r.zoom_by(4.0); // zoom = 4x → visible = 25% of image
        // Pan far right — crop should clamp to right edge, not get stuck at 0.9
        for _ in 0..30 { r.pan(1, 0); }
        // With a 100-pixel-wide image: visible_w=25, max_cx=75
        // pan_x should be ~3.0 now (30 * 0.1f32), which allows reaching max_cx.
        // Use 2.9 to avoid floating-point accumulation drift.
        assert!(r.pan_x >= 2.9);
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
