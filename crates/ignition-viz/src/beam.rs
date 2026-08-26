//! The volumetric beam: a Bevy `Material` wrapping `beam.wgsl`, plus the
//! cone mesh it draws on.
//!
//! One material asset per lit fixture, carrying that beam's own
//! parameters — ASLS Studio ships the same five values (`wpos`,
//! `direction`, `color`, `intensity`, `angle`) as per-instance attributes
//! on one shared instanced cylinder; a Bevy material is the same idea
//! with the engine doing the batching. Every term the shader evaluates is
//! world-space, so the mesh is free to be any shape at any orientation
//! and there is no custom vertex stage to keep in sync.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// Where `embedded_asset!` puts `beam.wgsl` — crate name, then the path
/// under `src/`.
const BEAM_SHADER: &str = "embedded://ignition_viz/beam.wgsl";

/// Registers the material and embeds its shader in the binary. Embedded
/// rather than loaded from an `assets/` directory so the visualizer runs
/// from any working directory — it is a console, not a game with a
/// content folder next to it.
pub struct BeamPlugin;

impl Plugin for BeamPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "beam.wgsl");
        app.add_plugins(MaterialPlugin::<BeamMaterial>::default());
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct BeamMaterial {
    /// The beam's colour, already dimmer-scaled, and NOT pre-attenuated —
    /// every falloff term is evaluated per-fragment.
    #[uniform(0)]
    pub color: LinearRgba,
    /// xyz = normalized world aim direction, w = beam **half** angle in
    /// degrees (see `beam.wgsl`, and `fixture_profile::beam_half_angle_deg`
    /// for why half).
    #[uniform(1)]
    pub direction_angle: Vec4,
    /// xyz = world position of the fixture's lens, w = throw distance.
    #[uniform(2)]
    pub origin_length: Vec4,
    /// x = haze, y = seconds, zw spare.
    #[uniform(3)]
    pub params: Vec4,
}

impl Material for BeamMaterial {
    fn fragment_shader() -> ShaderRef {
        BEAM_SHADER.into()
    }

    /// Additive, which in Bevy also implies no depth write and the
    /// transparent pass — so overlapping beams sum into each other rather
    /// than the nearest one hiding the rest, while still being occluded
    /// by the wall in front of them. Exactly ASLS's `AdditiveBlending` +
    /// `depthWrite: false`.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

impl BeamMaterial {
    pub fn new(
        color: LinearRgba,
        origin: Vec3,
        direction: Vec3,
        half_angle_deg: f32,
        length: f32,
        haze: f32,
    ) -> Self {
        let d = direction.normalize_or_zero();
        Self {
            color,
            direction_angle: Vec4::new(d.x, d.y, d.z, half_angle_deg),
            origin_length: Vec4::new(origin.x, origin.y, origin.z, length),
            params: Vec4::new(haze, 0.0, 0.0, 0.0),
        }
    }
}

/// The one cone mesh every beam in the rig draws on: a unit frustum, one
/// metre long, one metre in radius at the far end, narrow at the lens.
/// Each beam's real size comes from its `Transform`'s non-uniform scale,
/// so a rig of 70 fixtures is 70 entities sharing a single mesh asset
/// rather than 70 meshes rebuilt whenever a mover moves. That is the same
/// bargain ASLS strikes with one instanced cylinder widened per instance.
///
/// The near end is a small disc rather than a true point, so the lens
/// reads as an aperture rather than a degenerate vertex fan — ASLS fixes
/// theirs at a 9cm `BEAM_TOP_RADIUS`; as a fraction it stays sensible
/// across both a tight spot and a wide wash.
///
/// Skewing a cone by scaling it unevenly leaves its vertex normals wrong,
/// which would matter to a lit material and does not matter to this one:
/// `beam.wgsl` never reads the normal, only the world position.
pub fn beam_mesh() -> ConicalFrustum {
    ConicalFrustum {
        radius_top: 0.08,
        radius_bottom: 1.0,
        height: 1.0,
    }
}

/// Places and sizes `beam_mesh` so its narrow top cap sits at the lens
/// and its wide end lands `length` away down `direction`, `far_radius`
/// across. `ConicalFrustum`'s axis is +Y with the narrow cap at
/// `+height/2`, so the rotation maps -Y onto the aim.
pub fn beam_transform(origin: Vec3, direction: Vec3, length: f32, far_radius: f32) -> Transform {
    let d = direction.normalize_or_zero();
    Transform {
        translation: origin + d * (length * 0.5),
        rotation: Quat::from_rotation_arc(Vec3::NEG_Y, d),
        scale: Vec3::new(far_radius, length, far_radius),
    }
}

/// The volumetric shape of a linear source: a frustum with a rectangular
/// section, in the emitter's own frame — the near face is the strip
/// itself (`half_length` along X, `half_width` along Y, at z = 0), the
/// far face sits `length` down -Z and has opened by `half_angle_deg` on
/// every side. Where a point emitter's cone is one shared unit mesh
/// scaled per beam, a wedge's near face is a real size that no scale of
/// a unit mesh reproduces together with its spread, so each bar has its
/// own — rebuilt only when its throw changes.
///
/// Same material as the cone: `beam.wgsl` evaluates everything in world
/// space from the emitter's origin and aim, so it neither knows nor
/// cares what shape it is drawn on.
// r[impl viz.bar-emitters] - a box frustum for the shader the cone uses
pub fn wedge_mesh(half_length: f32, half_width: f32, length: f32, half_angle_deg: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let spread = length * half_angle_deg.to_radians().tan();
    let (nx, ny) = (half_length.max(0.001), half_width.max(0.001));
    let (fx, fy) = (nx + spread, ny + spread);
    let near = 0.0;
    let far = -length.max(0.01);
    // Near face 0..3, far face 4..7, each counter-clockwise seen from -Z.
    let positions: Vec<[f32; 3]> = vec![
        [-nx, -ny, near],
        [nx, -ny, near],
        [nx, ny, near],
        [-nx, ny, near],
        [-fx, -fy, far],
        [fx, -fy, far],
        [fx, fy, far],
        [-fx, fy, far],
    ];
    // Winding is immaterial: the material is additive and double-sided
    // by construction (see `beam.wgsl`'s clip-space one-siding), the
    // same as the cone it replaces.
    let indices: Vec<u32> = vec![
        0, 1, 5, 0, 5, 4, // -Y side
        1, 2, 6, 1, 6, 5, // +X side
        2, 3, 7, 2, 7, 6, // +Y side
        3, 0, 4, 3, 4, 7, // -X side
        4, 5, 6, 4, 6, 7, // far cap
        0, 2, 1, 0, 3, 2, // near cap (the strip)
    ];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; 8])
    .with_inserted_indices(Indices::U32(indices));
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let p = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("positions");
        p.iter().map(|v| Vec3::from(*v)).fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), v| (lo.min(v), hi.max(v)),
        )
    }

    // r[verify viz.bar-emitters] - the wedge's near face is the strip, its far face the spread
    #[test]
    fn the_wedge_starts_as_the_strip_and_opens_by_the_beam_angle() {
        let mesh = wedge_mesh(0.5, 0.03, 4.0, 20.0);
        let (lo, hi) = bounds(&mesh);
        let spread = 4.0 * 20f32.to_radians().tan();
        assert!(
            (hi.x - (0.5 + spread)).abs() < 1e-4,
            "far half-length {}",
            hi.x
        );
        assert!(
            (hi.y - (0.03 + spread)).abs() < 1e-4,
            "far half-width {}",
            hi.y
        );
        assert!(
            (hi.z - 0.0).abs() < 1e-6 && (lo.z + 4.0).abs() < 1e-6,
            "runs 0 .. -length"
        );
        // The near face is exactly the strip.
        let near: Vec<Vec3> = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .unwrap()
            .iter()
            .map(|v| Vec3::from(*v))
            .filter(|v| v.z.abs() < 1e-6)
            .collect();
        assert!(
            near.iter()
                .all(|v| (v.x.abs() - 0.5).abs() < 1e-6 && (v.y.abs() - 0.03).abs() < 1e-6)
        );
    }
}
