//! Offline export: a show rendered to a video file frame by frame
//! against the song's clock — `r[viz.export]`. This module is the
//! Bevy-free half: the frame schedule (which frame is which moment of
//! the song, deterministically, whatever the GPU is doing) and the sinks
//! frames are written to (an H.264 file through the `ffmpeg` feature, or
//! a PNG sequence into a directory without it). `app::run_export` owns
//! the render loop and calls in here.

use ignition_core::Bars;
use std::path::{Path, PathBuf};

/// What `viz --export` asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// The `.mp4` to write (with the `ffmpeg` feature), or the directory
    /// a PNG sequence goes into without it.
    pub path: PathBuf,
    pub from_bar: u32,
    pub to_bar: u32,
    pub fps: u32,
}

/// One moment of the export: the frame's index, its time from the start
/// of the export, and where in the song it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportFrame {
    pub index: u32,
    pub secs: f64,
    pub position: Bars,
}

/// The frames an export renders: `from_bar` (its first beat) up to but
/// not including `to_bar`, at `fps`, with the song at a constant `bpm`
/// and `beats_per_bar`. Pure arithmetic, so the same request always
/// yields the same frames.
// r[impl viz.export] - frame by frame against the song's clock
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSchedule {
    pub from_bar: u32,
    pub to_bar: u32,
    pub fps: u32,
    pub bpm: f64,
    pub beats_per_bar: u32,
}

impl FrameSchedule {
    pub fn new(request: &ExportRequest, bpm: f64) -> Self {
        Self {
            from_bar: request.from_bar,
            to_bar: request.to_bar,
            fps: request.fps.max(1),
            bpm: if bpm > 0.0 { bpm } else { 120.0 },
            beats_per_bar: 4,
        }
    }

    /// Seconds per frame.
    pub fn dt(&self) -> f64 {
        1.0 / self.fps as f64
    }

    /// Seconds one bar lasts.
    pub fn secs_per_bar(&self) -> f64 {
        self.beats_per_bar as f64 * 60.0 / self.bpm
    }

    /// How long the export runs, in seconds — zero for an empty range.
    pub fn duration_secs(&self) -> f64 {
        self.to_bar.saturating_sub(self.from_bar) as f64 * self.secs_per_bar()
    }

    /// How many frames there are: every frame whose time is inside the
    /// range, so the last one lands just short of `to_bar`.
    pub fn frame_count(&self) -> u32 {
        (self.duration_secs() * self.fps as f64).ceil().max(0.0) as u32
    }

    /// The song position `secs` into the export.
    pub fn position_at(&self, secs: f64) -> Bars {
        let beats = secs * self.bpm / 60.0;
        let bars = (beats / self.beats_per_bar as f64).floor();
        let beat = beats - bars * self.beats_per_bar as f64;
        Bars::new(self.from_bar + bars as u32, 1.0 + beat)
    }

    pub fn frames(&self) -> impl Iterator<Item = ExportFrame> + '_ {
        (0..self.frame_count()).map(|index| {
            let secs = index as f64 * self.dt();
            ExportFrame {
                index,
                secs,
                position: self.position_at(secs),
            }
        })
    }
}

/// Where rendered frames go. Frames arrive in order, as tightly packed
/// RGBA8 at the export's size.
pub trait FrameSink {
    fn push(
        &mut self,
        frame: &ExportFrame,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<()>;
    fn finish(self: Box<Self>) -> anyhow::Result<()>;
}

/// A numbered PNG per frame in a directory: `frame_000123.png`. The
/// fallback when the `ffmpeg` feature is off, and a perfectly good input
/// to `ffmpeg -framerate 30 -i frame_%06d.png` by hand.
// r[impl viz.export] - the no-codec fallback
pub struct PngSequence {
    dir: PathBuf,
}

impl PngSequence {
    pub fn create(dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    pub fn frame_path(&self, index: u32) -> PathBuf {
        self.dir.join(format!("frame_{index:06}.png"))
    }
}

impl FrameSink for PngSequence {
    fn push(
        &mut self,
        frame: &ExportFrame,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<()> {
        let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| anyhow::anyhow!("frame {} is not {width}x{height} RGBA", frame.index))?;
        // The alpha channel carries HDR brightness on an HDR camera;
        // a still wants the colour only.
        let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
        let path = self.frame_path(frame.index);
        rgb.save(&path)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))
    }

    fn finish(self: Box<Self>) -> anyhow::Result<()> {
        Ok(())
    }
}

/// The sink `request` wants: H.264 when the crate was built with
/// `ffmpeg`, otherwise a PNG sequence — into `request.path` as a
/// directory, or beside a `.mp4` path with the extension dropped.
// r[impl viz.export] - a video file with the codec, a frame sequence without
pub fn open_sink(
    request: &ExportRequest,
    width: u32,
    height: u32,
) -> anyhow::Result<Box<dyn FrameSink>> {
    #[cfg(feature = "ffmpeg")]
    {
        if request.path.extension().is_some() {
            return Ok(Box::new(h264::H264Sink::create(
                &request.path,
                width,
                height,
                request.fps,
            )?));
        }
    }
    let _ = (width, height);
    let dir = png_dir_for(&request.path);
    if dir != request.path {
        eprintln!(
            "viz: built without the ffmpeg feature; writing a PNG sequence to {}",
            dir.display()
        );
    }
    Ok(Box::new(PngSequence::create(&dir)?))
}

/// A `.mp4`-style path becomes a directory of the same name without
/// the extension; a bare path is already a directory.
pub fn png_dir_for(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
}

#[cfg(feature = "ffmpeg")]
pub mod h264 {
    //! H.264 in an MP4 through libav*, the same library the screen
    //! canvases decode with. Frames are converted RGBA -> YUV 4:2:0 by
    //! libswscale and encoded with libx264 at a quality-first CRF.

    use super::{ExportFrame, FrameSink};
    use ffmpeg::codec;
    use ffmpeg::format::Pixel;
    use ffmpeg::software::scaling;
    use ffmpeg::util::frame::video::Video as VideoFrame;
    use ffmpeg::{Dictionary, Rational};
    use ffmpeg_next as ffmpeg;
    use std::path::Path;

    pub struct H264Sink {
        output: ffmpeg::format::context::Output,
        encoder: codec::encoder::video::Encoder,
        scaler: scaling::Context,
        stream_index: usize,
        stream_time_base: Rational,
        width: u32,
        height: u32,
    }

    impl H264Sink {
        // r[impl viz.export] - the video file
        pub fn create(path: &Path, width: u32, height: u32, fps: u32) -> anyhow::Result<Self> {
            ffmpeg::init().map_err(|e| anyhow::anyhow!("ffmpeg init: {e}"))?;
            let mut output = ffmpeg::format::output(path)
                .map_err(|e| anyhow::anyhow!("opening {} for writing: {e}", path.display()))?;
            let codec = codec::encoder::find(codec::Id::H264)
                .ok_or_else(|| anyhow::anyhow!("this ffmpeg has no H.264 encoder"))?;
            let global_header = output
                .format()
                .flags()
                .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
            let mut stream = output
                .add_stream(codec)
                .map_err(|e| anyhow::anyhow!("adding the video stream: {e}"))?;
            let stream_index = stream.index();
            let mut video = codec::context::Context::new_with_codec(codec)
                .encoder()
                .video()
                .map_err(|e| anyhow::anyhow!("video encoder context: {e}"))?;
            // x264 wants even dimensions for 4:2:0.
            let width = width & !1;
            let height = height & !1;
            video.set_width(width);
            video.set_height(height);
            video.set_format(Pixel::YUV420P);
            video.set_time_base(Rational(1, fps as i32));
            video.set_frame_rate(Some(Rational(fps as i32, 1)));
            if global_header {
                video.set_flags(codec::Flags::GLOBAL_HEADER);
            }
            let mut options = Dictionary::new();
            options.set("preset", "medium");
            options.set("crf", "18");
            let encoder = video
                .open_with(options)
                .map_err(|e| anyhow::anyhow!("opening the H.264 encoder: {e}"))?;
            stream.set_parameters(&encoder);
            stream.set_time_base(Rational(1, fps as i32));
            let stream_time_base = stream.time_base();
            output
                .write_header()
                .map_err(|e| anyhow::anyhow!("writing the container header: {e}"))?;
            let scaler = scaling::Context::get(
                Pixel::RGBA,
                width,
                height,
                Pixel::YUV420P,
                width,
                height,
                scaling::Flags::BILINEAR,
            )
            .map_err(|e| anyhow::anyhow!("colour conversion context: {e}"))?;
            Ok(Self {
                output,
                encoder,
                scaler,
                stream_index,
                stream_time_base,
                width,
                height,
            })
        }

        fn drain(&mut self) -> anyhow::Result<()> {
            let mut packet = ffmpeg::Packet::empty();
            while self.encoder.receive_packet(&mut packet).is_ok() {
                packet.set_stream(self.stream_index);
                packet.rescale_ts(self.encoder.time_base(), self.stream_time_base);
                packet
                    .write_interleaved(&mut self.output)
                    .map_err(|e| anyhow::anyhow!("writing a packet: {e}"))?;
            }
            Ok(())
        }
    }

    impl FrameSink for H264Sink {
        fn push(
            &mut self,
            frame: &ExportFrame,
            rgba: &[u8],
            width: u32,
            height: u32,
        ) -> anyhow::Result<()> {
            let mut src = VideoFrame::new(Pixel::RGBA, self.width, self.height);
            let src_stride = (width * 4) as usize;
            let dst_stride = src.stride(0);
            let rows = self.height.min(height) as usize;
            let row_bytes = (self.width * 4) as usize;
            for y in 0..rows {
                let from = &rgba[y * src_stride..y * src_stride + row_bytes];
                src.data_mut(0)[y * dst_stride..y * dst_stride + row_bytes].copy_from_slice(from);
            }
            let mut dst = VideoFrame::new(Pixel::YUV420P, self.width, self.height);
            self.scaler
                .run(&src, &mut dst)
                .map_err(|e| anyhow::anyhow!("converting frame {}: {e}", frame.index))?;
            dst.set_pts(Some(frame.index as i64));
            self.encoder
                .send_frame(&dst)
                .map_err(|e| anyhow::anyhow!("encoding frame {}: {e}", frame.index))?;
            self.drain()
        }

        fn finish(mut self: Box<Self>) -> anyhow::Result<()> {
            self.encoder
                .send_eof()
                .map_err(|e| anyhow::anyhow!("flushing the encoder: {e}"))?;
            self.drain()?;
            self.output
                .write_trailer()
                .map_err(|e| anyhow::anyhow!("writing the container trailer: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(from_bar: u32, to_bar: u32, fps: u32) -> ExportRequest {
        ExportRequest {
            path: PathBuf::from("out.mp4"),
            from_bar,
            to_bar,
            fps,
        }
    }

    /// r[verify viz.export] - the schedule is exact arithmetic on the song's clock
    #[test]
    fn the_frame_schedule_walks_the_bars_at_the_song_tempo() {
        // 120 BPM in 4/4: a bar is two seconds; bars 9 and 10 are four
        // seconds, 120 frames at 30 fps.
        let schedule = FrameSchedule::new(&request(9, 11, 30), 120.0);
        assert_eq!(schedule.frame_count(), 120);
        let frames: Vec<ExportFrame> = schedule.frames().collect();
        assert_eq!(frames.len(), 120);
        assert_eq!(frames[0].position, Bars::new(9, 1.0));
        assert!((frames[30].secs - 1.0).abs() < 1e-9);
        assert_eq!(frames[30].position, Bars::new(9, 3.0));
        assert_eq!(frames[60].position, Bars::new(10, 1.0));
        let last = frames[119];
        assert!(last.secs < schedule.duration_secs());
        assert_eq!(last.position.bar, 10);
        assert!(last.position.beat < 5.0);
        // Consecutive frames are exactly one dt apart.
        for pair in frames.windows(2) {
            assert!((pair[1].secs - pair[0].secs - schedule.dt()).abs() < 1e-9);
        }
    }

    /// r[verify viz.export] - an empty or inverted range renders nothing, and odd rates round up
    #[test]
    fn degenerate_ranges_and_partial_frames_are_handled() {
        assert_eq!(
            FrameSchedule::new(&request(5, 5, 30), 120.0).frame_count(),
            0
        );
        assert_eq!(
            FrameSchedule::new(&request(7, 5, 30), 120.0).frame_count(),
            0
        );
        // 100 BPM: one bar is 2.4 s, 24 fps -> 57.6 frames, the partial one kept.
        let schedule = FrameSchedule::new(&request(1, 2, 24), 100.0);
        assert_eq!(schedule.frame_count(), 58);
        // A zero fps or bpm is not a division by zero.
        let schedule = FrameSchedule::new(&request(1, 2, 0), 0.0);
        assert_eq!(schedule.fps, 1);
        assert_eq!(schedule.frame_count(), 2);
    }

    #[test]
    fn a_png_sequence_numbers_its_frames_and_a_video_path_becomes_a_directory() {
        assert_eq!(
            png_dir_for(Path::new("renders/take1.mp4")),
            PathBuf::from("renders/take1")
        );
        assert_eq!(
            png_dir_for(Path::new("renders/take1")),
            PathBuf::from("renders/take1")
        );
        let dir = std::env::temp_dir().join(format!("ig-export-{}", std::process::id()));
        let mut sink = PngSequence::create(&dir).unwrap();
        assert_eq!(sink.frame_path(7), dir.join("frame_000007.png"));
        let frame = ExportFrame {
            index: 7,
            secs: 0.0,
            position: Bars::bar(1),
        };
        sink.push(&frame, &[255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
            .unwrap();
        assert!(dir.join("frame_000007.png").exists());
        assert!(
            sink.push(&frame, &[0; 3], 2, 1).is_err(),
            "a short buffer is an error"
        );
        Box::new(sink).finish().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
