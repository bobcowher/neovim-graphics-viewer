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
