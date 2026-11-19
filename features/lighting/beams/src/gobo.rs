//! The gobo texture: one layer per gobo, with its mips.
//!
//! A gobo in the air is blurred by picking a mip level (focus, frost, and
//! the stretch of ray one sample stands for), so the mips are part of the
//! picture and are built here by box filter, not left to chance.

use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};

/// One gobo's picture: `size` × `size` coverage values, row by row, 0
/// where the gobo blocks the light and 255 where it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoboLayer {
    size: u32,
    texels: Vec<u8>,
}

impl GoboLayer {
    /// A gobo `size` texels square (rounded up to a power of two), drawn by
    /// `open`: given a point in the gobo's unit disc (x and y from −1 to
    /// 1), is the gobo open there? Each texel is the average of a 4 × 4
    /// grid of points, so edges are antialiased.
    #[must_use]
    pub fn draw(size: u16, open: impl Fn(f32, f32) -> bool) -> Self {
        Self::sample(size, |x, y| if open(x, y) { 1.0 } else { 0.0 })
    }

    /// As [`GoboLayer::draw`], from how open the gobo is at each point, 0
    /// to 1: for a gobo that is already a picture.
    #[must_use]
    pub fn sample(size: u16, open: impl Fn(f32, f32) -> f32) -> Self {
        const GRID: u8 = 4;
        const POINTS: f32 = 16.0;
        let size = size.max(1).next_power_of_two();
        let scale = 2.0 / (f32::from(size) * f32::from(GRID));
        let coordinate = |texel: u16, sub: u8| {
            let point = f32::from(texel).mul_add(f32::from(GRID), f32::from(sub));
            (point + 0.5).mul_add(scale, -1.0)
        };
        let texels = (0..size)
            .flat_map(|row| (0..size).map(move |column| (row, column)))
            .map(|(row, column)| {
                let total: f32 = (0..GRID)
                    .flat_map(|sy| (0..GRID).map(move |sx| (sx, sy)))
                    .map(|(sx, sy)| {
                        open(coordinate(column, sx), coordinate(row, sy)).clamp(0.0, 1.0)
                    })
                    .sum();
                coverage_byte(total / POINTS)
            })
            .collect();
        Self {
            size: u32::from(size),
            texels,
        }
    }

    /// A layer that blocks nothing: what a beam with no gobo would sample.
    #[must_use]
    pub fn open(size: u16) -> Self {
        Self::draw(size, |_, _| true)
    }

    /// The layer at half the size, each texel the mean of four.
    fn halved(&self) -> Self {
        let size = (self.size / 2).max(1);
        let at = |x: u32, y: u32| {
            let index = usize::try_from(y.saturating_mul(self.size).saturating_add(x)).ok();
            u16::from(index.and_then(|i| self.texels.get(i)).copied().unwrap_or(0))
        };
        let texels = (0..size)
            .flat_map(|y| (0..size).map(move |x| (x.saturating_mul(2), y.saturating_mul(2))))
            .map(|(x, y)| {
                let sum = at(x, y)
                    .saturating_add(at(x.saturating_add(1), y))
                    .saturating_add(at(x, y.saturating_add(1)))
                    .saturating_add(at(x.saturating_add(1), y.saturating_add(1)));
                u8::try_from(sum.saturating_add(2) / 4).unwrap_or(u8::MAX)
            })
            .collect();
        Self { size, texels }
    }

    /// The layer and every mip below it, largest first, as one run of
    /// bytes.
    fn with_mips(&self) -> Vec<u8> {
        let mut bytes = self.texels.clone();
        let mut level = self.clone();
        while level.size > 1 {
            level = level.halved();
            bytes.extend_from_slice(&level.texels);
        }
        bytes
    }
}

/// A coverage of 0 to 1 as a byte.
fn coverage_byte(coverage: f32) -> u8 {
    let scaled = (coverage.clamp(0.0, 1.0) * 255.0).round();
    (0..=u8::MAX)
        .find(|&byte| f32::from(byte) >= scaled)
        .unwrap_or(u8::MAX)
}

/// The gobo texture for `layers`, which must all be the same size: an
/// `R8Unorm` array with full mip chains, sampled trilinearly and clamped
/// at its edge. `None` when there are no layers or their sizes differ.
#[must_use]
pub fn gobo_image(layers: &[GoboLayer]) -> Option<Image> {
    let size = layers.first()?.size;
    if layers.iter().any(|layer| layer.size != size) {
        return None;
    }
    let data: Vec<u8> = layers.iter().flat_map(GoboLayer::with_mips).collect();
    let mut image = Image::new_uninit(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: u32::try_from(layers.len()).ok()?,
        },
        TextureDimension::D2,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = size.ilog2().saturating_add(1);
    image.data = Some(data);
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..Default::default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mipmap_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_layer_is_white_at_every_mip() {
        let bytes = GoboLayer::open(8).with_mips();
        assert_eq!(bytes.len(), 64 + 16 + 4 + 1);
        assert!(bytes.iter().all(|&byte| byte == 255));
    }

    #[test]
    fn half_a_gobo_is_half_bright_at_its_smallest_mip() {
        let layer = GoboLayer::draw(16, |x, _| x < 0.0);
        let bytes = layer.with_mips();
        let last = bytes.last().copied().unwrap();
        assert!((126..=129).contains(&last), "smallest mip is {last}");
    }

    #[test]
    fn edges_are_antialiased() {
        // A diagonal edge crosses texels, which must come out part open.
        let layer = GoboLayer::draw(16, |x, y| x + y < 0.01);
        assert!(layer.texels.iter().any(|&t| t > 0 && t < 255));
    }

    #[test]
    fn the_image_holds_every_layer_and_mip() {
        let image =
            gobo_image(&[GoboLayer::open(16), GoboLayer::draw(16, |x, _| x < 0.0)]).unwrap();
        assert_eq!(image.texture_descriptor.mip_level_count, 5);
        assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 2);
        assert_eq!(image.data.unwrap().len(), 2 * (256 + 64 + 16 + 4 + 1));
    }

    #[test]
    fn layers_of_different_sizes_are_refused() {
        assert!(gobo_image(&[GoboLayer::open(16), GoboLayer::open(8)]).is_none());
        assert!(gobo_image(&[]).is_none());
    }
}
