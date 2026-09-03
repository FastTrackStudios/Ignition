//! The Vidvox HAP backend — pure Rust, no system library.
//!
//! HAP is what this corner of the industry actually plays: Resolume,
//! VDMX, `TouchDesigner` and every media server that has to hold frame
//! rate on a wall of screens ship it, because a HAP frame is not an
//! encoded picture at all — it is a block-compressed GPU texture with a
//! Snappy wrapper. There is no motion compensation, no B-frames and no
//! reference chain, which buys two things that matter more here than
//! file size:
//!
//! * **Every frame is independent.** A seek is exact and costs one frame
//!   of work, so scrubbing the song scrubs the video with it. h.264 has
//!   to decode forward from the nearest keyframe, which is why a scrub
//!   through an mp4 lands *near* where you asked.
//! * **It decodes at wall speed.** The expensive part of a conventional
//!   codec is not there to do.
//!
//! The one thing this backend does not yet do is take the free half of
//! that bargain: it decompresses `BCn` to RGBA on the CPU so the frame
//! goes up the same texture path a still does. Uploading the `BCn` blocks
//! straight to the GPU is what HAP is *for*, and it is a bigger change
//! than this — Hap Q stores scaled `YCoCg`, so it needs a shader pass and
//! therefore a material of its own rather than `StandardMaterial`. Worth
//! doing; not needed to get moving pictures on the wall.

use super::{Decoder, Frame, Meta, VideoError};
use hap_qt::{QtHapReader, TextureFormat};
use std::path::Path;

pub(super) fn open(path: &Path) -> Result<(Meta, Box<dyn Decoder>), VideoError> {
    let reader = QtHapReader::open(path).map_err(|e| VideoError::Backend(e.to_string()))?;
    let (width, height) = reader.resolution();
    let frames = reader.frame_count();
    if frames == 0 {
        return Err(VideoError::Empty);
    }
    // hap-qt derives one rate for the track rather than exposing the
    // per-sample deltas. Every HAP file in practice comes out of an
    // exporter at a constant rate, and a wrong-by-a-hair rate shows up
    // as drift against the song rather than as a broken picture.
    let fps = f64::from(reader.fps()).max(1.0);
    tracing::info!(
        path = %path.display(),
        codec = reader.codec_type(),
        width,
        height,
        frames,
        fps,
        "video: opened a HAP clip"
    );

    let meta = Meta {
        width,
        height,
        duration: reader.duration(),
    };
    Ok((
        meta,
        Box::new(HapDecoder {
            reader,
            width,
            height,
            frames,
            fps,
            next: 0,
        }),
    ))
}

struct HapDecoder {
    reader: QtHapReader,
    width: u32,
    height: u32,
    frames: u32,
    fps: f64,
    next: u32,
}

impl Decoder for HapDecoder {
    fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
        if self.next >= self.frames {
            return Ok(None);
        }
        let index = self.next;
        self.next = self.next.saturating_add(1);
        let frame = self
            .reader
            .read_frame(index)
            .map_err(|e| VideoError::Backend(format!("reading frame {index}: {e}")))?;
        Ok(Some(Frame {
            pts: f64::from(index) / self.fps,
            rgba: to_rgba(&frame, self.width, self.height)?,
            generation: 0,
        }))
    }

    /// Exact, unlike every other codec's: an all-intra format has no
    /// keyframe to decode forward from, so the frame asked for is the
    /// frame read.
    fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        let index = crate::num::u32_of_f64((secs * self.fps).round());
        self.next = index.min(self.frames.saturating_sub(1));
        Ok(())
    }
}

/// `BCn` blocks to RGBA8.
fn to_rgba(frame: &hap_qt::HapFrame, width: u32, height: u32) -> Result<Vec<u8>, VideoError> {
    let format = match frame.format {
        TextureFormat::RgbDxt1 => texpresso::Format::Bc1,
        TextureFormat::RgbaDxt5 | TextureFormat::YcoCgDxt5 => texpresso::Format::Bc3,
        // BC7 and BC4 are formats texpresso does not decompress. They
        // are also the two that would be free through a GPU upload, so
        // the fix is that path rather than a software decoder for them.
        other => {
            return Err(VideoError::Backend(format!(
                "{other:?} needs a GPU-side decode, which this backend does not do yet"
            )));
        }
    };

    let (w, h) = (
        crate::num::usize_of_u32(width),
        crate::num::usize_of_u32(height),
    );
    let mut rgba = vec![0u8; w.saturating_mul(h).saturating_mul(4)];
    format.decompress(&frame.data, w, h, &mut rgba);
    if frame.format.needs_ycocg_convert() {
        ycocg_to_rgb(&mut rgba);
    }
    Ok(rgba)
}

/// Hap Q's scaled `YCoCg`, in place.
///
/// DXT5 gives four channels of very different quality: the three colour
/// channels are interpolated together and the alpha channel on its own,
/// with more precision. Hap Q exploits that by putting luma — the part
/// the eye is fussiest about — in alpha, and the two chroma differences
/// in red and green, scaled by a per-texel factor parked in blue. It is
/// the same trick as putting luma at full resolution in any other codec,
/// done inside a texture format that has no idea it is happening.
///
/// Normally this runs in a shader on the way to the screen. Here it is
/// the price of decoding on the CPU — see the module note.
fn ycocg_to_rgb(rgba: &mut [u8]) {
    for texel in rgba.as_chunks_mut::<4>().0 {
        let co = f32::from(texel[0]) / 255.0 - 0.5;
        let cg = f32::from(texel[1]) / 255.0 - 0.5;
        // Blue carries the scale the encoder divided the chroma by,
        // 1..=32 packed into 0..=255.
        let scale = (f32::from(texel[2]) / 255.0).mul_add(31.875, 1.0);
        let (co, cg) = (co / scale, cg / scale);
        let y = f32::from(texel[3]) / 255.0;
        let to_u8 = |v: f32| crate::num::byte_of_f32(v.clamp(0.0, 1.0) * 255.0);
        texel[0] = to_u8(y + co - cg);
        texel[1] = to_u8(y + cg);
        texel[2] = to_u8(y - co - cg);
        // Hap Q has no alpha of its own; Hap Q Alpha carries it in a
        // second BC4 plane this backend does not read yet.
        texel[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A neutral grey: no chroma difference, so every channel comes
    /// back at the luma.
    ///
    /// Within a code value, because neutral chroma is 0.5 and 0.5 is not
    /// an 8-bit code — 128 is half a step above it, which is a hair of
    /// green in the answer and is in every HAP Q file ever encoded.
    #[test]
    fn ycocg_leaves_grey_alone() {
        let mut texel = [128, 128, 0, 200];
        ycocg_to_rgb(&mut texel);
        for (channel, value) in texel[..3].iter().enumerate() {
            assert!(
                i32::from(*value).abs_diff(200) <= 1,
                "channel {channel} came back {value}, not grey"
            );
        }
        assert_eq!(texel[3], 255);
    }

    /// Chroma is divided by the scale in blue, so the same Co/Cg pair
    /// means less colour at a higher scale. Getting this backwards
    /// produces a picture that is merely over-saturated, which is why it
    /// is worth pinning rather than eyeballing.
    #[test]
    fn a_higher_scale_means_less_chroma() {
        let mut low = [200, 128, 0, 128];
        let mut high = [200, 128, 255, 128];
        ycocg_to_rgb(&mut low);
        ycocg_to_rgb(&mut high);
        let spread = |t: [u8; 4]| i32::from(t[0]) - i32::from(t[2]);
        assert!(
            spread(low) > spread(high),
            "low {low:?} was not more saturated than high {high:?}"
        );
    }

    /// Needs a real HAP file, so it builds one first — hap-qt writes as
    /// well as reads, which is the only reason this can be tested at all
    /// without committing a video into the repo.
    #[test]
    #[ignore = "writes a temporary HAP file"]
    fn reads_back_a_written_clip() {
        use hap_qt::{CompressionMode, HapFormat, HapFrameEncoder, QtHapWriter, VideoConfig};

        let path = std::env::temp_dir().join("ignition-hap-roundtrip.mov");
        let (w, h) = (64u32, 64u32);
        let mut encoder = HapFrameEncoder::new(HapFormat::Hap1, w, h).expect("encoder");
        encoder.set_compression(CompressionMode::Snappy);
        let config = VideoConfig::new(w, h, 10.0, HapFormat::Hap1);
        let mut writer = QtHapWriter::create(&path, config).expect("writer");
        for i in 0..10u32 {
            // A flat colour per frame, so a decoded frame says which
            // frame it is.
            let level = crate::num::u8_of_u32(i.saturating_mul(20));
            let rgba: Vec<u8> = (0..w * h).flat_map(|_| [level, 0, 0, 255]).collect();
            let frame = encoder.encode(&rgba).expect("encode");
            writer.write_frame(&frame).expect("write");
        }
        writer.finalize().expect("finalize");

        let (meta, mut decoder) = open(&path).expect("open");
        assert_eq!((meta.width, meta.height), (w, h));
        let first = decoder.next_frame().expect("decode").expect("a frame");
        assert_eq!(
            crate::num::u32_of_usize(first.rgba.len()),
            w.saturating_mul(h).saturating_mul(4)
        );
        decoder.seek(0.5).expect("seek");
        let mid = decoder.next_frame().expect("decode").expect("a frame");
        assert!(mid.pts >= 0.45, "seek landed at {}", mid.pts);
        assert!(
            mid.rgba[0] > first.rgba[0],
            "the seek did not move: {} vs {}",
            first.rgba[0],
            mid.rgba[0]
        );
        let _ = std::fs::remove_file(&path);
    }
}
