//! A beam, as the visualizer describes it and as the GPU is handed it.

use bevy::{
    color::{ColorToComponents, LinearRgba},
    ecs::component::Component,
    math::{Vec3, Vec4},
    render::render_resource::ShaderType,
    transform::components::{GlobalTransform, Transform},
};

use crate::math::{Profile, peak_candela};

/// One beam in the air: put it on an entity with a [`Transform`]. The lens
/// is at the entity's origin and the beam shines along its −Z, as a Bevy
/// `SpotLight` does, so the two can share an entity.
///
/// A prism is as many beams as it has facets, each with the facet's share
/// of [`Beam::flux`] and its own direction.
#[derive(Component, Clone, Debug, PartialEq)]
#[require(Transform)]
pub struct Beam {
    /// The fixture's GDTF beam angle (to half intensity) at the current
    /// zoom, the full angle in radians.
    pub beam_angle: f32,
    /// Its field angle (to a tenth), the full angle in radians.
    pub field_angle: f32,
    /// The radius of the front lens in metres (GDTF `BeamRadius`).
    pub lens_radius: f32,
    /// How far the beam reaches from the lens, in metres. It fades out
    /// before it gets there; a surface nearer than this cuts it for free.
    pub range: f32,
    /// What the fixture puts out at full, in lumens (GDTF `LuminousFlux`).
    pub flux: f32,
    /// The beam's colour.
    pub colour: LinearRgba,
    /// 0 to 1: the dimmer, with the strobe's shutter folded in.
    pub dimmer: f32,
    /// The gobo in the beam, if any.
    pub gobo: Option<Gobo>,
    /// Frost, 0 to 1: widens the field and softens the gobo.
    pub frost: f32,
    /// How strongly this beam shows in the haze, 1 as the room's haze has
    /// it. A lever for a look, not physics.
    pub haze_response: f32,
    /// Where this beam's own light starts to compress, in exposed units.
    /// Looking along a beam integrates a long bright path; the knee keeps
    /// that from blowing out against the beams seen from the side.
    pub knee: f32,
}

/// A gobo in a beam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gobo {
    /// Its layer in the plugin's gobo texture (`BeamGobos`).
    pub slot: u16,
    /// Its rotation about the beam's axis, in radians.
    pub rotation: f32,
    /// How far out of focus it is, as mip levels of blur.
    pub blur: f32,
}

impl Default for Beam {
    fn default() -> Self {
        Self {
            beam_angle: 0.26,
            field_angle: 0.35,
            lens_radius: 0.06,
            range: 40.0,
            flux: 10_000.0,
            colour: LinearRgba::WHITE,
            dimmer: 1.0,
            gobo: None,
            frost: 0.0,
            haze_response: 1.0,
            knee: 3.0,
        }
    }
}

/// How much frost at full widens the field angle.
const FROST_SPREAD: f32 = 1.6;
/// How many mip levels frost at full blurs the gobo by.
const FROST_BLUR: f32 = 4.0;
/// The fraction of its range at which a beam starts fading out.
const FADE_START: f32 = 0.6;

/// One beam in the storage buffer: `Beam` in `shaders/scatter.wesl`,
/// member for member. All `vec4`, so the layout has no padding to get
/// wrong on either side.
#[derive(ShaderType, Clone, Copy, Debug, PartialEq)]
pub struct GpuBeam {
    pub lens_range: Vec4,
    pub axis_tan_edge: Vec4,
    pub right_x_beam: Vec4,
    pub up_lens_radius: Vec4,
    pub colour_candela: Vec4,
    pub profile_haze_fade: Vec4,
    pub gobo_knee: Vec4,
    /// Where its occluders start in their buffer (x), how many (y).
    pub occluders: Vec4,
}

impl GpuBeam {
    /// The beam's cone, as the shader builds it.
    #[must_use]
    pub fn cone(&self) -> crate::math::Cone {
        crate::math::Cone::from_lens(
            self.lens_range.truncate(),
            self.axis_tan_edge.truncate(),
            self.up_lens_radius.w,
            self.axis_tan_edge.w,
            self.lens_range.w,
        )
    }
}

impl Beam {
    /// The beam's shape across its width, with the tangent of its edge:
    /// its beam and field angles, the field widened by frost.
    #[must_use]
    pub fn profile(&self) -> (Profile, f32) {
        let spread = self.frost.clamp(0.0, 1.0).mul_add(FROST_SPREAD - 1.0, 1.0);
        Profile::from_half_angles(self.beam_angle * 0.5, self.field_angle * 0.5 * spread)
    }

    /// The beam for the GPU, or `None` when it puts no light in the air.
    ///
    /// The on-axis intensity is the flux over the solid angle the profile
    /// spreads it across, so zooming in brightens the beam by exactly what
    /// it narrows.
    #[must_use]
    pub fn to_gpu(&self, transform: &GlobalTransform) -> Option<GpuBeam> {
        let level = self.dimmer.clamp(0.0, 1.0) * self.flux;
        if level <= 0.0 || self.haze_response <= 0.0 || self.range <= 0.0 {
            return None;
        }
        let (profile, tan_edge) = self.profile();
        let candela = peak_candela(level, &profile, tan_edge);
        let (axis, right, up) = (
            Vec3::from(transform.forward()),
            Vec3::from(transform.right()),
            Vec3::from(transform.up()),
        );
        let frost_blur = self.frost.clamp(0.0, 1.0) * FROST_BLUR;
        let gobo = self.gobo.map_or(Vec3::new(-1.0, 0.0, 0.0), |gobo| {
            Vec3::new(f32::from(gobo.slot), gobo.rotation, gobo.blur + frost_blur)
        });
        Some(GpuBeam {
            lens_range: transform.translation().extend(self.range),
            axis_tan_edge: axis.extend(tan_edge),
            right_x_beam: right.extend(profile.x_beam),
            up_lens_radius: up.extend(self.lens_radius.max(1.0e-3)),
            colour_candela: self.colour.to_vec3().extend(candela),
            profile_haze_fade: Vec4::new(
                profile.exponent,
                profile.x_field,
                self.haze_response,
                FADE_START,
            ),
            gobo_knee: gobo.extend(self.knee.max(1.0e-3)),
            occluders: Vec4::ZERO,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dark_beam_is_not_sent() {
        let dark = Beam {
            dimmer: 0.0,
            ..Beam::default()
        };
        assert_eq!(dark.to_gpu(&GlobalTransform::IDENTITY), None);
    }

    #[test]
    fn the_beam_shines_along_minus_z_from_its_origin() {
        let transform = GlobalTransform::from(Transform::from_xyz(1.0, 6.0, -2.0));
        let gpu = Beam::default().to_gpu(&transform).unwrap();
        assert_eq!(gpu.lens_range.truncate(), Vec3::new(1.0, 6.0, -2.0));
        assert!((gpu.axis_tan_edge.truncate() - Vec3::NEG_Z).length() < 1.0e-6);
        assert_eq!(gpu.gobo_knee.x, -1.0);
    }

    #[test]
    fn the_dimmer_scales_the_candela_and_nothing_else() {
        let full = Beam::default().to_gpu(&GlobalTransform::IDENTITY).unwrap();
        let half = Beam {
            dimmer: 0.5,
            ..Beam::default()
        }
        .to_gpu(&GlobalTransform::IDENTITY)
        .unwrap();
        assert!((half.colour_candela.w / full.colour_candela.w - 0.5).abs() < 1.0e-5);
        assert_eq!(half.axis_tan_edge, full.axis_tan_edge);
        assert_eq!(half.profile_haze_fade, full.profile_haze_fade);
    }

    #[test]
    fn frost_widens_the_field_and_dims_the_axis() {
        let clear = Beam::default().to_gpu(&GlobalTransform::IDENTITY).unwrap();
        let frosted = Beam {
            frost: 1.0,
            ..Beam::default()
        }
        .to_gpu(&GlobalTransform::IDENTITY)
        .unwrap();
        assert!(frosted.axis_tan_edge.w > clear.axis_tan_edge.w * 1.4);
        assert!(frosted.colour_candela.w < clear.colour_candela.w);
    }
}
