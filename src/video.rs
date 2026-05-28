use ffmpeg_next as ffmpeg;
use std::time::{Duration, Instant};

pub struct VideoDecoder {
    input_ctx: ffmpeg::format::context::Input,
    video_stream_idx: usize,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: ffmpeg::software::scaling::Context,
    time_base: ffmpeg::Rational,
    width: u32,
    height: u32,
    pub playing: bool,
    pub finished: bool,
    current_pts: i64,
    flushed: bool,
    stream_epoch: Instant,
    epoch_set: bool,
    pub duration_secs: f64,
}

impl VideoDecoder {
    pub fn open(path: &str) -> Result<Self, String> {
        ffmpeg::init().map_err(|e| format!("ffmpeg init: {e}"))?;

        let input_ctx = ffmpeg::format::input(&path)
            .map_err(|e| format!("open {path}: {e}"))?;

        let raw_duration = input_ctx.duration();
        let duration_secs = if raw_duration > 0 {
            raw_duration as f64 / ffmpeg::ffi::AV_TIME_BASE as f64
        } else {
            0.0
        };

        let (video_stream_idx, time_base, codec_ctx) = {
            let stream = input_ctx
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or_else(|| format!("no video stream in {path}"))?;

            let idx = stream.index();
            let tb = stream.time_base();
            let ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|e| format!("codec context: {e}"))?;
            (idx, tb, ctx)
        };

        let decoder = codec_ctx.decoder().video()
            .map_err(|e| format!("video decoder: {e}"))?;

        let width = decoder.width();
        let height = decoder.height();

        let scaler = ffmpeg::software::scaling::Context::get(
            decoder.format(),
            width,
            height,
            ffmpeg::format::Pixel::RGBA,
            width,
            height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        ).map_err(|e| format!("scaler: {e}"))?;

        Ok(Self {
            input_ctx,
            video_stream_idx,
            decoder,
            scaler,
            time_base,
            width,
            height,
            playing: true,
            finished: false,
            current_pts: 0,
            flushed: false,
            stream_epoch: Instant::now(),
            epoch_set: false,
            duration_secs,
        })
    }

    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut frame = ffmpeg::frame::Video::empty();

        loop {
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {
                    if let Some(pts) = frame.pts() {
                        self.current_pts = pts;
                        if !self.epoch_set {
                            let tb = f64::from(self.time_base);
                            let pts_secs = pts as f64 * tb;
                            self.stream_epoch = Instant::now()
                                .checked_sub(Duration::from_secs_f64(pts_secs))
                                .unwrap_or_else(Instant::now);
                            self.epoch_set = true;
                        }
                    }
                    return Ok(Some(self.to_rgba(&frame)?));
                }
                Err(ffmpeg::Error::Eof) => {
                    self.finished = true;
                    self.playing = false;
                    return Ok(None);
                }
                Err(_) => {
                    // EAGAIN or similar: decoder needs more packets — fall through to feed one
                }
            }

            // Feed the next video packet; skip non-video packets (audio, subtitles)
            if !self.flushed {
                loop {
                    match self.input_ctx.packets().next() {
                        Some((stream, packet)) => {
                            if stream.index() == self.video_stream_idx {
                                self.decoder.send_packet(&packet)
                                    .map_err(|e| format!("send_packet: {e}"))?;
                                break;
                            }
                        }
                        None => {
                            self.decoder.send_eof()
                                .map_err(|e| format!("send_eof: {e}"))?;
                            self.flushed = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Seek relative to current position by delta_secs seconds.
    pub fn seek(&mut self, delta_secs: i32) -> Result<(), String> {
        let tb = f64::from(self.time_base);
        let delta_ts = (delta_secs as f64 / tb) as i64;
        let target_ts = (self.current_pts + delta_ts).max(0);

        // AVSEEK_FLAG_BACKWARD = 1: seek to nearest keyframe at or before target
        let flags: i32 = if delta_secs < 0 { 1 } else { 0 };
        unsafe {
            ffmpeg::ffi::av_seek_frame(
                self.input_ctx.as_mut_ptr(),
                self.video_stream_idx as i32,
                target_ts,
                flags,
            );
        }
        self.decoder.flush();
        self.finished = false;
        self.flushed = false;
        self.epoch_set = false;
        Ok(())
    }

    /// Seek to the beginning and resume playback.
    pub fn rewind(&mut self) -> Result<(), String> {
        unsafe {
            ffmpeg::ffi::av_seek_frame(
                self.input_ctx.as_mut_ptr(),
                -1,  // any stream
                0,   // timestamp 0 = start
                1,   // AVSEEK_FLAG_BACKWARD
            );
        }
        self.decoder.flush();
        self.current_pts = 0;
        self.finished = false;
        self.flushed = false;
        self.epoch_set = false;
        self.playing = true;
        Ok(())
    }

    pub fn toggle_play(&mut self) {
        if self.finished {
            let _ = self.rewind();
        } else if self.playing {
            self.playing = false;
        } else {
            self.playing = true;
            self.stream_epoch = Instant::now()
                .checked_sub(Duration::from_secs_f64(self.position_secs()))
                .unwrap_or_else(Instant::now);
            self.epoch_set = true;
        }
    }

    pub fn is_playing(&self) -> bool { self.playing }
    pub fn is_finished(&self) -> bool { self.finished }
    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    pub fn current_display_time(&self) -> Instant {
        let tb = f64::from(self.time_base);
        let secs = self.current_pts as f64 * tb;
        self.stream_epoch + Duration::from_secs_f64(secs)
    }

    pub fn position_secs(&self) -> f64 {
        self.current_pts as f64 * f64::from(self.time_base)
    }

    fn to_rgba(&mut self, frame: &ffmpeg::frame::Video) -> Result<Vec<u8>, String> {
        let mut rgb = ffmpeg::frame::Video::empty();
        self.scaler.run(frame, &mut rgb)
            .map_err(|e| format!("scale: {e}"))?;

        let w = self.width as usize;
        let h = self.height as usize;
        let stride = rgb.stride(0);
        let data = rgb.data(0);
        let mut pixels = Vec::with_capacity(w * h * 4);

        for y in 0..h {
            let row = y * stride;
            let len = w * 4;
            pixels.extend_from_slice(&data[row..row + len]);
        }
        Ok(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_nonexistent_returns_err() {
        let result = VideoDecoder::open("/nonexistent/video.mp4");
        assert!(result.is_err());
        let msg = result.err().unwrap();
        assert!(msg.contains("/nonexistent/video.mp4"), "error should mention path, got: {msg}");
    }
}
