//! The libav backend — everything HAP is not.
//!
//! HAP is the right format for a wall of screens, but it is not the
//! format anybody's clip arrives in. Content comes off a phone, a stock
//! library or a render as h.264 in an mp4, and telling an operator to
//! transcode before they can see it on the model is a good way to have
//! nobody use the model. So: ffmpeg for the general case, HAP for the
//! case that matters at showtime.
//!
//! The cost is a system library — `pkgs.ffmpeg` in `flake.nix` — which
//! is exactly why this is a feature and not a dependency.
//!
//! Note the asymmetry with HAP on a scrub. An inter-frame codec can
//! only start at a keyframe, so landing on an arbitrary time means
//! decoding every frame from the last keyframe up to it and throwing
//! them away. That is done below and it is exact — but it costs up to a
//! whole GOP of work per scrub, where HAP costs one frame. It is the
//! argument for putting show content in HAP even when the source was an
//! mp4.

use super::{Decoder, Frame, Meta, VideoError};
use ffmpeg_next as ffmpeg;
use std::path::Path;

fn init() -> Result<(), VideoError> {
    // `av_register_all` is gone in modern ffmpeg, but ffmpeg-next's
    // `init` still sets up logging and the device list, and it is not
    // safe to call concurrently.
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut result = Ok(());
    ONCE.call_once(|| {
        if let Err(e) = ffmpeg::init() {
            result = Err(VideoError::Backend(format!("initialising ffmpeg: {e}")));
        }
    });
    result
}

pub(super) fn open(path: &Path) -> Result<(Meta, Box<dyn Decoder>), VideoError> {
    init()?;
    let input = ffmpeg::format::input(path).map_err(backend)?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| VideoError::Backend(format!("{} has no video track", path.display())))?;
    let index = stream.index();
    // Seconds per stream timestamp unit, folded to an f64 once: every
    // decoded frame's pts goes through it, and `Rational` arithmetic per
    // frame buys nothing when the answer feeds a comparison against a
    // song position.
    let time_base =
        f64::from(stream.time_base().numerator()) / f64::from(stream.time_base().denominator());

    let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(backend)?
        .decoder()
        .video()
        .map_err(backend)?;
    let (width, height) = (decoder.width(), decoder.height());

    // Same size in and out: this is a pixel-format conversion (YUV to
    // RGBA), not a resize, so the interpolation flag never gets to do
    // anything.
    let scaler = ffmpeg::software::scaling::Context::get(
        decoder.format(),
        width,
        height,
        ffmpeg::format::Pixel::RGBA,
        width,
        height,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )
    .map_err(backend)?;

    // The container's duration, in ffmpeg's own microsecond time base
    // rather than the stream's.
    let duration = input.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);
    tracing::info!(
        path = %path.display(),
        width,
        height,
        duration,
        "video: opened a clip through ffmpeg"
    );

    Ok((
        Meta {
            width,
            height,
            duration: duration.max(0.0),
        },
        Box::new(FfmpegDecoder {
            input,
            decoder,
            scaler: Scaler(scaler),
            index,
            time_base,
            width,
            height,
            drained: false,
            target: None,
        }),
    ))
}

/// Half a frame at 500 fps: enough slack that a timestamp that rounds a
/// hair under the time asked for still counts as having reached it.
const FRAME_EPSILON: f64 = 0.001;

fn backend(error: ffmpeg::Error) -> VideoError {
    VideoError::Backend(error.to_string())
}

/// A `SwsContext`, moved to the decoder thread.
///
/// ffmpeg-next holds it as a raw pointer and so leaves it `!Send`, which
/// makes the whole decoder `!Send` and it never leaves the ground. The
/// pointer is moved to the worker thread once, at construction, and
/// touched by nothing else for the rest of its life — the one shape
/// `Send` describes and an auto-trait cannot infer through a raw
/// pointer. (It is *not* `Sync`, and must not become so: libswscale
/// contexts genuinely cannot be scaled through from two threads.)
struct Scaler(ffmpeg::software::scaling::Context);

unsafe impl Send for Scaler {}

struct FfmpegDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    scaler: Scaler,
    index: usize,
    time_base: f64,
    width: u32,
    height: u32,
    /// Whether the end of the file has already been pushed through the
    /// decoder. Sending EOF twice is an error, and the decoder holds
    /// several frames past the last packet — so "no more packets" is not
    /// yet "no more frames".
    drained: bool,
    /// The time a seek asked for, while frames before it are still
    /// being decoded past — see `next_frame`.
    target: Option<f64>,
}

impl Decoder for FfmpegDecoder {
    fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
        loop {
            if let Some(frame) = self.receive()? {
                // Throw away everything between the keyframe ffmpeg
                // landed on and the time actually asked for.
                //
                // Without this a seek does not visibly move at all on a
                // clip with sparse keyframes — a three-second x264
                // export can have exactly one, at frame 0, so every
                // seek "succeeds" and every frame after it comes from
                // the top of the file. The cost is decoding up to a
                // GOP's worth of frames nobody sees, which is what every
                // other player does too and is the honest price of an
                // inter-frame codec.
                if let Some(target) = self.target {
                    if frame.pts + FRAME_EPSILON < target {
                        continue;
                    }
                    self.target = None;
                }
                return Ok(Some(frame));
            }
            // `s.index()` before the tuple goes out of scope: the stream
            // handle borrows the input context, and the packet does not,
            // so taking the number here is what lets the decoder be fed
            // on the next line.
            let next = self.input.packets().next().map(|(s, p)| (s.index(), p));
            match next {
                Some((stream, packet)) => {
                    if stream == self.index {
                        self.decoder.send_packet(&packet).map_err(backend)?;
                    }
                }
                // End of the clip. Anything still being skipped
                // towards is past the end, so the request goes with it.
                None if self.drained => {
                    self.target = None;
                    return Ok(None);
                }
                None => {
                    self.drained = true;
                    self.decoder.send_eof().map_err(backend)?;
                }
            }
        }
    }

    /// Positions the container at the keyframe at or before `secs`;
    /// `next_frame` decodes forward from there and drops what it passes,
    /// so the frame that comes out is the one asked for.
    fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        let ts = (secs.max(0.0) * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
        self.input.seek(ts, ..ts).map_err(backend)?;
        // Without this the decoder keeps emitting what it had buffered
        // from before the jump, which looks like the scrub being ignored
        // for a few frames.
        self.decoder.flush();
        self.drained = false;
        self.target = Some(secs.max(0.0));
        Ok(())
    }
}

impl FfmpegDecoder {
    /// One frame out of the decoder, converted to RGBA, or `None` when
    /// it wants more packets first.
    fn receive(&mut self) -> Result<Option<Frame>, VideoError> {
        let mut decoded = ffmpeg::frame::Video::empty();
        // Every error here means "nothing to give you yet" (EAGAIN) or
        // "nothing ever again" (EOF). Both are answered by feeding more
        // packets, and when there are none left `next_frame`'s `drained`
        // flag ends the clip — so there is no error case worth telling
        // apart.
        if self.decoder.receive_frame(&mut decoded).is_err() {
            return Ok(None);
        }

        let mut rgba_frame = ffmpeg::frame::Video::empty();
        self.scaler
            .0
            .run(&decoded, &mut rgba_frame)
            .map_err(backend)?;

        // ffmpeg pads each row out to its own alignment, so the buffer
        // is stride-by-height and not width-by-height. Handing that
        // straight to the GPU produces the classic diagonal shear.
        let stride = rgba_frame.stride(0);
        let row = self.width as usize * 4;
        let data = rgba_frame.data(0);
        let mut rgba = Vec::with_capacity(row * self.height as usize);
        for y in 0..self.height as usize {
            let start = y * stride;
            rgba.extend_from_slice(&data[start..start + row]);
        }

        // `timestamp` is the best-effort one: a stream whose frames
        // carry no pts still has to land somewhere on the song's clock.
        let pts = decoded.timestamp().unwrap_or(0) as f64 * self.time_base;
        Ok(Some(Frame {
            pts,
            rgba,
            generation: 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes a real clip, named by `IGNITION_TEST_CLIP`.
    ///
    /// Unlike the HAP backend, there is no way to write the fixture from
    /// here — encoding h.264 needs an encoder this build does not link —
    /// and a video file is not something to commit. So the clip is
    /// supplied:
    ///
    /// ```text
    /// ffmpeg -f lavfi -i testsrc2=size=480x270:rate=30:duration=3 \
    ///        -c:v libx264 -pix_fmt yuv420p /tmp/clip.mp4
    /// IGNITION_TEST_CLIP=/tmp/clip.mp4 \
    ///   cargo test -p ignition-viz --features ffmpeg -- --ignored
    /// ```
    #[test]
    #[ignore = "needs a clip in IGNITION_TEST_CLIP"]
    fn decodes_a_supplied_clip() {
        let Ok(clip) = std::env::var("IGNITION_TEST_CLIP") else {
            panic!("set IGNITION_TEST_CLIP to an mp4");
        };
        let (meta, mut decoder) = open(Path::new(&clip)).expect("open");
        assert!(meta.width > 0 && meta.height > 0);

        let first = decoder.next_frame().expect("decode").expect("a frame");
        assert_eq!(
            first.rgba.len(),
            meta.width as usize * meta.height as usize * 4,
            "a row of padding survived the stride copy"
        );

        // Forward, then back: the backwards case is the one a scrub
        // makes and the one a decoder that only ever runs forwards gets
        // wrong.
        decoder.seek(meta.duration * 0.5).expect("seek");
        let middle = decoder.next_frame().expect("decode").expect("a frame");
        assert!(middle.pts > first.pts, "seek went nowhere");
        decoder.seek(0.0).expect("seek");
        let back = decoder.next_frame().expect("decode").expect("a frame");
        assert!(back.pts < middle.pts, "the clip would not rewind");
    }
}
