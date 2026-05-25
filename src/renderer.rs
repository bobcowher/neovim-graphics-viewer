use image::DynamicImage;

pub struct Renderer {
    image: Option<DynamicImage>,
    pub frame_pixels: Option<(Vec<u32>, u32, u32)>,
    pub zoom_level: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Renderer {
    pub fn new() -> Self {
        Self { image: None, frame_pixels: None, zoom_level: 1.0, pan_x: 0.0, pan_y: 0.0 }
    }

    pub fn load(&mut self, path: &str) -> Result<(), String> {
        let img = image::open(path).map_err(|e| format!("failed to decode {path}: {e}"))?;
        self.image = Some(img);
        self.frame_pixels = None;
        self.zoom_level = 1.0;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        Ok(())
    }

    pub fn update_frame(&mut self, pixels: Vec<u32>, width: u32, height: u32) {
        self.frame_pixels = Some((pixels, width, height));
        self.image = None;
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

    pub fn render(&self, buffer: &mut [u32], width: u32, height: u32) {
        if let Some((ref pixels, img_w, img_h)) = self.frame_pixels {
            self.blit(
                |x, y| pixels[(y * img_w + x) as usize],
                img_w, img_h, buffer, width, height,
            );
        } else if let Some(ref img) = self.image {
            let rgba = img.to_rgba8();
            let (img_w, img_h) = rgba.dimensions();
            self.blit(
                |x, y| {
                    let p = rgba.get_pixel(x, y).0;
                    ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
                },
                img_w, img_h, buffer, width, height,
            );
        } else {
            buffer.iter_mut().for_each(|p| *p = 0x1a1a2e);
        }
    }

    fn blit(
        &self,
        src: impl Fn(u32, u32) -> u32,
        img_w: u32,
        img_h: u32,
        buffer: &mut [u32],
        width: u32,
        height: u32,
    ) {
        let fit_scale = (width as f32 / img_w as f32).min(height as f32 / img_h as f32);
        let eff = fit_scale * self.zoom_level;
        let cx = (width as f32 - img_w as f32 * eff) / 2.0 + self.pan_x;
        let cy = (height as f32 - img_h as f32 * eff) / 2.0 + self.pan_y;

        for py in 0..height {
            for px in 0..width {
                let sx = ((px as f32 - cx) / eff) as i32;
                let sy = ((py as f32 - cy) / eff) as i32;
                buffer[(py * width + px) as usize] = if sx >= 0
                    && sx < img_w as i32
                    && sy >= 0
                    && sy < img_h as i32
                {
                    src(sx as u32, sy as u32)
                } else {
                    0x1a1a2e
                };
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

    #[test]
    fn update_frame_stores_pixels() {
        let mut r = Renderer::new();
        let pixels = vec![0x00FF0000u32; 4];
        r.update_frame(pixels.clone(), 2, 2);
        assert!(r.frame_pixels.is_some());
        let (stored, w, h) = r.frame_pixels.as_ref().unwrap();
        assert_eq!(*w, 2);
        assert_eq!(*h, 2);
        assert_eq!(*stored, pixels);
    }

    #[test]
    fn load_failure_does_not_clear_frame_pixels() {
        let mut r = Renderer::new();
        r.update_frame(vec![0u32; 4], 2, 2);
        let _ = r.load("/nonexistent.png");
        assert!(r.frame_pixels.is_some());
    }

    #[test]
    fn update_frame_clears_image() {
        let mut r = Renderer::new();
        r.update_frame(vec![0u32; 9], 3, 3);
        assert!(r.image.is_none());
        assert!(r.frame_pixels.is_some());
    }

    #[test]
    fn render_frame_pixels_fills_buffer() {
        let mut r = Renderer::new();
        r.update_frame(vec![0x00FF0000u32], 1, 1);
        let mut buf = vec![0u32; 4];
        r.render(&mut buf, 2, 2);
        assert!(buf.iter().all(|&p| p == 0x00FF0000));
    }
}
