use ffmpeg_next as ffmpeg;
use std::time::Duration;

pub struct VideoDecoder {
    input_ctx: ffmpeg::format::context::Input,
    video_stream_idx: usize,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: ffmpeg::software::scaling::Context,
    time_base: ffmpeg::Rational,
    width: u32,
    height: u32,
    frame_duration: Duration,
    pub playing: bool,
    pub finished: bool,
    current_pts: i64,
}

impl VideoDecoder {
    pub fn open(path: &str) -> Result<Self, String> {
        ffmpeg::init().map_err(|e| format!("ffmpeg init: {e}"))?;

        let input_ctx = ffmpeg::format::input(&path)
            .map_err(|e| format!("open {path}: {e}"))?;

        let (video_stream_idx, time_base, frame_duration, codec_ctx) = {
            let stream = input_ctx
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or_else(|| format!("no video stream in {path}"))?;

            let idx = stream.index();
            let tb = stream.time_base();
            let r = stream.rate();
            let fps = if r.numerator() == 0 { 30.0_f64 }
                      else { r.numerator() as f64 / r.denominator() as f64 };
            let dur = Duration::from_secs_f64(1.0 / fps);
            let ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|e| format!("codec context: {e}"))?;
            (idx, tb, dur, ctx)
        };

        let decoder = codec_ctx.decoder().video()
            .map_err(|e| format!("video decoder: {e}"))?;

        let width = decoder.width();
        let height = decoder.height();

        let scaler = ffmpeg::software::scaling::Context::get(
            decoder.format(),
            width,
            height,
            ffmpeg::format::Pixel::RGB24,
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
            frame_duration,
            playing: true,
            finished: false,
            current_pts: 0,
        })
    }

    /// Decode and return the next video frame as XRGB pixels.
    /// Returns Ok(None) at end-of-file (sets finished=true, playing=false).
    pub fn next_frame(&mut self) -> Result<Option<Vec<u32>>, String> {
        let mut frame = ffmpeg::frame::Video::empty();

        loop {
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {
                    if let Some(pts) = frame.pts() {
                        self.current_pts = pts;
                    }
                    return Ok(Some(self.to_xrgb(&frame)?));
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
            loop {
                match self.input_ctx.packets().next() {
                    Some((stream, packet)) => {
                        if stream.index() == self.video_stream_idx {
                            self.decoder.send_packet(&packet)
                                .map_err(|e| format!("send_packet: {e}"))?;
                            break;
                        }
                        // Non-video packet — read next
                    }
                    None => {
                        // File EOF — flush buffered frames
                        self.decoder.send_eof()
                            .map_err(|e| format!("send_eof: {e}"))?;
                        break;
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
        self.playing = true;
        Ok(())
    }

    /// Toggle play/pause. If the video has finished, rewind and play.
    pub fn toggle_play(&mut self) {
        if self.finished {
            let _ = self.rewind();
        } else {
            self.playing = !self.playing;
        }
    }

    pub fn is_playing(&self) -> bool { self.playing }
    pub fn is_finished(&self) -> bool { self.finished }
    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }
    pub fn frame_duration(&self) -> Duration { self.frame_duration }

    fn to_xrgb(&mut self, frame: &ffmpeg::frame::Video) -> Result<Vec<u32>, String> {
        let mut rgb = ffmpeg::frame::Video::empty();
        self.scaler.run(frame, &mut rgb)
            .map_err(|e| format!("scale: {e}"))?;

        let w = self.width as usize;
        let h = self.height as usize;
        let stride = rgb.stride(0);
        let data = rgb.data(0);
        let mut pixels = Vec::with_capacity(w * h);

        for y in 0..h {
            for x in 0..w {
                let i = y * stride + x * 3;
                pixels.push(
                    ((data[i] as u32) << 16)
                        | ((data[i + 1] as u32) << 8)
                        | data[i + 2] as u32,
                );
            }
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
