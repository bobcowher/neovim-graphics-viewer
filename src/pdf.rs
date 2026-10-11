use std::process::Command;

use image::RgbaImage;

/// Render resolution for pages. 150 DPI keeps text sharp up to ~2x zoom at
/// typical window sizes while rendering a page in well under 200ms.
const RENDER_DPI: u32 = 150;
/// Rendered pages kept in memory (~8MB each at 150 DPI letter/A4).
const CACHE_PAGES: usize = 5;
/// Long-side cap for the frame sent to the terminal; it scales up the rest.
pub const MAX_VIEW_EDGE: u32 = 1600;
/// Background for the area around the page.
const BG: [u8; 4] = [40, 40, 40, 255];

pub struct PdfDoc {
    path: String,
    pub page_count: u32,
    /// 1-based current page.
    pub page: u32,
    cache: Vec<(u32, RgbaImage)>,
}

impl PdfDoc {
    pub fn open(path: &str) -> Result<Self, String> {
        let out = run_tool("pdfinfo", Command::new("pdfinfo").arg(path))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("failed to open {path}: {}", stderr.trim()));
        }
        let page_count = parse_page_count(&String::from_utf8_lossy(&out.stdout))
            .filter(|&n| n > 0)
            .ok_or_else(|| format!("failed to read page count of {path}"))?;
        Ok(Self { path: path.to_string(), page_count, page: 1, cache: Vec::new() })
    }

    /// Move by `delta` pages, clamped to the document. Returns true if the page changed.
    pub fn step(&mut self, delta: i32) -> bool {
        self.set_page(self.page as i64 + delta as i64)
    }

    /// Jump to a 1-based page; negative counts from the end (-1 = last).
    pub fn goto(&mut self, page: i32) -> bool {
        let target = if page < 0 { self.page_count as i64 + 1 + page as i64 } else { page as i64 };
        self.set_page(target)
    }

    fn set_page(&mut self, target: i64) -> bool {
        let clamped = target.clamp(1, self.page_count as i64) as u32;
        let changed = clamped != self.page;
        self.page = clamped;
        changed
    }

    pub fn current_image(&mut self) -> Result<&RgbaImage, String> {
        let page = self.page;
        if let Some(i) = self.cache.iter().position(|(p, _)| *p == page) {
            return Ok(&self.cache[i].1);
        }
        let img = render_page(&self.path, page)?;
        if self.cache.len() >= CACHE_PAGES {
            self.cache.remove(0);
        }
        self.cache.push((page, img));
        Ok(&self.cache.last().unwrap().1)
    }
}

fn run_tool(name: &str, cmd: &mut Command) -> Result<std::process::Output, String> {
    cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{name} not found: install poppler-utils for PDF support")
        } else {
            format!("{name}: {e}")
        }
    })
}

fn render_page(path: &str, page: u32) -> Result<RgbaImage, String> {
    // No output root: pdftoppm writes the PNG to stdout.
    let out = run_tool(
        "pdftoppm",
        Command::new("pdftoppm")
            .args(["-f", &page.to_string(), "-l", &page.to_string()])
            .args(["-r", &RENDER_DPI.to_string(), "-png"])
            .arg(path),
    )?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("failed to render page {page} of {path}: {}", stderr.trim()));
    }
    image::load_from_memory(&out.stdout)
        .map(|img| img.to_rgba8())
        .map_err(|e| format!("failed to decode page {page} of {path}: {e}"))
}

pub fn parse_page_count(pdfinfo_out: &str) -> Option<u32> {
    pdfinfo_out
        .lines()
        .find_map(|l| l.strip_prefix("Pages:"))
        .and_then(|v| v.trim().parse().ok())
}

/// Viewer state for a page: zoom relative to fit-width, and scroll position
/// as a fraction (0 = top/left, 1 = bottom/right) of the scrollable range.
/// Fractions survive window resizes and page changes without recomputation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfView {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for PdfView {
    fn default() -> Self {
        Self { zoom: 1.0, x: 0.0, y: 0.0 }
    }
}

impl PdfView {
    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
    }

    /// Scroll by `dx`/`dy` steps of 10% of the viewport, for a page of
    /// `page_w`x`page_h` shown in a `vw`x`vh` viewport. Returns false if the
    /// view was already at the edge (or the page fits), so callers can turn
    /// the page instead.
    pub fn scroll(&mut self, dx: i32, dy: i32, page_w: u32, page_h: u32, vw: u32, vh: u32) -> bool {
        let before = *self;
        let (scaled_w, scaled_h) = scaled_size(self.zoom, page_w, page_h, vw);
        let range_x = scaled_w - vw as f32;
        let range_y = scaled_h - vh as f32;
        if range_x > 0.0 {
            self.x = (self.x + dx as f32 * 0.1 * vw as f32 / range_x).clamp(0.0, 1.0);
        }
        if range_y > 0.0 {
            self.y = (self.y + dy as f32 * 0.1 * vh as f32 / range_y).clamp(0.0, 1.0);
        }
        *self != before
    }
}

/// Page size after scaling its width to `zoom` x viewport width.
fn scaled_size(zoom: f32, page_w: u32, page_h: u32, vw: u32) -> (f32, f32) {
    let scaled_w = vw as f32 * zoom;
    (scaled_w, page_h as f32 * scaled_w / page_w.max(1) as f32)
}

/// Build a `vw`x`vh` frame showing `page` scaled to `zoom` x viewport width,
/// offset by the view's scroll fractions. Pages smaller than the viewport are centered.
pub fn compose(page: &RgbaImage, view: &PdfView, vw: u32, vh: u32) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(vw, vh, image::Rgba(BG));
    let (pw, ph) = page.dimensions();
    let (scaled_w, scaled_h) = scaled_size(view.zoom, pw, ph, vw);
    let scale = scaled_w / pw.max(1) as f32;

    let vis_w = scaled_w.min(vw as f32);
    let vis_h = scaled_h.min(vh as f32);
    let off_x = view.x * (scaled_w - vw as f32).max(0.0);
    let off_y = view.y * (scaled_h - vh as f32).max(0.0);

    // Visible region mapped back to source pixels; crop first so a deep zoom
    // never resizes the whole page.
    let sx = ((off_x / scale) as u32).min(pw - 1);
    let sy = ((off_y / scale) as u32).min(ph - 1);
    let sw = ((vis_w / scale).round() as u32).clamp(1, pw - sx);
    let sh = ((vis_h / scale).round() as u32).clamp(1, ph - sy);
    let region = image::imageops::crop_imm(page, sx, sy, sw, sh).to_image();

    let dw = (vis_w.round() as u32).max(1);
    let dh = (vis_h.round() as u32).max(1);
    let scaled = image::imageops::resize(&region, dw, dh, image::imageops::FilterType::Triangle);

    let dx = (vw.saturating_sub(dw) / 2) as i64;
    let dy = (vh.saturating_sub(dh) / 2) as i64;
    image::imageops::replace(&mut canvas, &scaled, dx, dy);
    canvas
}

/// Viewport size in pixels for a window of `cols`x`rows` cells of
/// `cell_w`x`cell_h` pixels, capped to MAX_VIEW_EDGE on the long side.
pub fn viewport_px(cols: u32, rows: u32, cell_w: f32, cell_h: f32) -> (u32, u32) {
    let w = (cols as f32 * cell_w).max(1.0);
    let h = (rows as f32 * cell_h).max(1.0);
    let long = w.max(h);
    let k = if long > MAX_VIEW_EDGE as f32 { MAX_VIEW_EDGE as f32 / long } else { 1.0 };
    (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/two-pages.pdf");

    fn solid(w: u32, h: u32, px: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba(px))
    }

    #[test]
    fn parse_page_count_reads_pages_line() {
        let out = "Title:  x\nPages:           42\nEncrypted:  no\n";
        assert_eq!(parse_page_count(out), Some(42));
    }

    #[test]
    fn parse_page_count_missing_is_none() {
        assert_eq!(parse_page_count("Title: x\n"), None);
    }

    #[test]
    fn open_fixture_counts_pages() {
        let doc = PdfDoc::open(FIXTURE).unwrap();
        assert_eq!(doc.page_count, 2);
        assert_eq!(doc.page, 1);
    }

    #[test]
    fn open_bad_path_errors_with_path() {
        let err = PdfDoc::open("/nonexistent/doc.pdf").err().expect("should fail");
        assert!(err.contains("/nonexistent/doc.pdf"), "got: {err}");
    }

    #[test]
    fn render_page_produces_image() {
        let mut doc = PdfDoc::open(FIXTURE).unwrap();
        let img = doc.current_image().unwrap();
        assert!(img.width() > 100 && img.height() > 100);
    }

    #[test]
    fn step_and_goto_clamp() {
        let mut doc = PdfDoc::open(FIXTURE).unwrap();
        assert!(!doc.step(-1));
        assert_eq!(doc.page, 1);
        assert!(doc.step(1));
        assert_eq!(doc.page, 2);
        assert!(!doc.step(5));
        assert_eq!(doc.page, 2);
        assert!(doc.goto(1));
        assert_eq!(doc.page, 1);
        assert!(doc.goto(-1));
        assert_eq!(doc.page, 2);
        doc.goto(99);
        assert_eq!(doc.page, 2);
    }

    #[test]
    fn viewport_uses_cell_pixels() {
        assert_eq!(viewport_px(80, 24, 10.0, 20.0), (800, 480));
    }

    #[test]
    fn viewport_caps_long_edge_preserving_aspect() {
        let (w, h) = viewport_px(400, 50, 10.0, 20.0); // 4000x1000
        assert_eq!(w, MAX_VIEW_EDGE);
        assert_eq!(h, MAX_VIEW_EDGE / 4);
    }

    #[test]
    fn compose_output_matches_viewport() {
        let page = solid(100, 200, [255, 255, 255, 255]);
        let out = compose(&page, &PdfView::default(), 300, 150);
        assert_eq!(out.dimensions(), (300, 150));
    }

    #[test]
    fn compose_fit_width_fills_horizontally() {
        // Page scaled to viewport width: no background at the left/right edges.
        let page = solid(100, 200, [255, 255, 255, 255]);
        let out = compose(&page, &PdfView::default(), 300, 150);
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(299, 149).0, [255, 255, 255, 255]);
    }

    #[test]
    fn compose_zoomed_out_centers_with_background() {
        let page = solid(100, 100, [255, 255, 255, 255]);
        let view = PdfView { zoom: 0.5, x: 0.0, y: 0.0 };
        let out = compose(&page, &view, 200, 200);
        assert_eq!(out.get_pixel(0, 0).0, BG);
        assert_eq!(out.get_pixel(100, 100).0, [255, 255, 255, 255]);
    }

    #[test]
    fn compose_scroll_bottom_shows_end_of_page() {
        // Top half black, bottom half white; scrolled to the bottom we see white.
        let mut page = solid(100, 400, [0, 0, 0, 255]);
        for y in 200..400 { for x in 0..100 { page.put_pixel(x, y, image::Rgba([255, 255, 255, 255])); } }
        let top = compose(&page, &PdfView::default(), 100, 100);
        assert_eq!(top.get_pixel(50, 50).0, [0, 0, 0, 255]);
        let bottom = compose(&page, &PdfView { zoom: 1.0, x: 0.0, y: 1.0 }, 100, 100);
        assert_eq!(bottom.get_pixel(50, 50).0, [255, 255, 255, 255]);
    }

    #[test]
    fn scroll_moves_by_viewport_steps_and_clamps() {
        // Scaled page 100x400 in a 100x100 viewport: 300px scrollable, step 10px.
        let mut v = PdfView::default();
        v.scroll(0, 3, 100, 400, 100, 100);
        assert!((v.y - 0.1).abs() < 1e-4, "y={}", v.y);
        v.scroll(0, -100, 100, 400, 100, 100);
        assert_eq!(v.y, 0.0);
        v.scroll(0, 1000, 100, 400, 100, 100);
        assert_eq!(v.y, 1.0);
    }

    #[test]
    fn scroll_noop_when_page_fits() {
        let mut v = PdfView::default();
        v.scroll(1, 1, 100, 50, 100, 100);
        assert_eq!(v, PdfView::default());
    }

    #[test]
    fn scroll_reports_movement() {
        let mut v = PdfView::default();
        assert!(v.scroll(0, 1, 100, 400, 100, 100), "mid-page scroll moves");
        v.y = 1.0;
        assert!(!v.scroll(0, 1, 100, 400, 100, 100), "at bottom, down is a no-op");
        assert!(v.scroll(0, -1, 100, 400, 100, 100), "at bottom, up moves");
        v.y = 0.0;
        assert!(!v.scroll(0, -1, 100, 400, 100, 100), "at top, up is a no-op");
    }

    #[test]
    fn scroll_reports_no_movement_when_page_fits() {
        let mut v = PdfView::default();
        assert!(!v.scroll(0, 1, 100, 50, 100, 100));
    }

    #[test]
    fn zoom_clamps() {
        let mut v = PdfView::default();
        v.zoom_by(100.0);
        assert!(v.zoom <= 8.0);
        v.zoom_by(0.0001);
        assert!(v.zoom >= 0.1);
    }
}
