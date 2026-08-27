//! Spike: Bevy Solari — raytraced direct and indirect lighting in place
//! of the shadow-map budget. Built only with `--features solari`.
//!
//! What Solari is, as 0.19 ships it: a ReSTIR DI + GI pass over a
//! deferred G-buffer, tracing rays against a TLAS of every
//! `RaytracingMesh3d`. Its light sources are **emissive meshes and
//! directional lights only** — `scene/binder.rs` binds nothing else.
//! Bevy's `SpotLight`s and `PointLight`s are not in its light list, and
//! because a `SolariLighting` camera is marked `SkipDeferredLighting`,
//! the PBR deferred pass that *would* have lit the opaque scene with
//! them is skipped. Under Solari a rig of 67 spots lights nothing.
//!
//! So this plugin does the thing that would be needed to make it work
//! at all: every fixture spill gets an emissive disc (an area light) in
//! the raytracing scene, sized to the lens and driven each frame from
//! the spill's `SpotLight` colour and intensity, with a black snoot
//! tube behind it that exists only in the raytracing scene, to give the
//! Lambertian disc something like the fixture's cone. Neither is drawn
//! by the raster pass. See docs/research/solari-spike.md for what came
//! of it.
//!
//! The shadow-map budget is left untouched: the haze camera still
//! marches Bevy's volumetric fog, which reads the spot lights and their
//! shadow maps, and Solari has no fog of its own.

use crate::haze::HazeCamera;
use crate::spawn::FixtureSpill;
use bevy::camera::CameraMainTextureUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};

/// The device features Solari needs, for a host that makes the device
/// itself (the studio, the bench). `SolariPlugins::required_wgpu_features`
/// undersells it: the scene bind group is a *storage* binding array
/// and the shaders take their constants as immediates, neither of
/// which is in that list (Bevy's own device asks for every feature the
/// adapter has, so it never notices). In wgpu 29 `EXPERIMENTAL_RAY_QUERY`
/// carries acceleration-structure support with it.
pub fn required_features() -> wgpu::Features {
    SolariPlugins::required_wgpu_features()
        | wgpu::Features::STORAGE_RESOURCE_BINDING_ARRAY
        | wgpu::Features::IMMEDIATES
        // Its passes put GPU timestamps inside a compute pass, which the
        // encoder-level timestamp feature the bench has does not cover.
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
}

/// The limits Solari needs: WebGPU's defaults have every binding-array
/// and acceleration-structure limit at zero and eight storage buffers a
/// stage where the lighting pass binds fourteen, so the device takes
/// the adapter's limits. `INDIRECT_FIRST_INSTANCE` is deliberately not
/// requested, so Bevy's GPU culling — measured slower here — stays off.
pub fn widen_limits(_limits: wgpu::Limits, adapter: &wgpu::Limits) -> wgpu::Limits {
    adapter.clone()
}

/// Adds the Solari plugins and the systems that put the venue into the
/// raytracing scene.
pub struct SolariPlugin;

impl Plugin for SolariPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SolariPlugins).add_systems(
            Update,
            (tag_meshes, tag_cameras, spawn_area_lights, drive_area_lights),
        );
    }
}

/// Every opaque mesh with a standard material joins the raytracing
/// scene. Solari only builds a BLAS for a mesh with exactly position,
/// normal, uv0 and tangent, indexed u32 — a primitive from Bevy's
/// builders has no tangents and u16 or u32 indices depending on size,
/// so both are fixed up here; a mesh with vertex colours or a second UV
/// set is left out and said so.
fn tag_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    added: Query<
        (Entity, &Mesh3d, Option<&Name>),
        (
            With<MeshMaterial3d<StandardMaterial>>,
            Without<RaytracingMesh3d>,
            Without<NotRaytraced>,
            Without<crate::haze::HazeComposite>,
        ),
    >,
) {
    for (entity, mesh3d, name) in &added {
        let Some(mut mesh) = meshes.get_mut(&mesh3d.0) else {
            continue;
        };
        match mesh.indices() {
            Some(Indices::U16(i)) => {
                let wide: Vec<u32> = i.iter().map(|&i| u32::from(i)).collect();
                mesh.insert_indices(Indices::U32(wide));
            }
            Some(Indices::U32(_)) => {}
            // A triangle soup: index it as it stands.
            None => {
                let n = mesh.count_vertices() as u32;
                mesh.insert_indices(Indices::U32((0..n).collect()));
            }
        }
        if !mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT)
            && mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL)
            && mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0)
            && let Err(e) = mesh.generate_tangents()
        {
            warn!("solari: {:?}: no tangents ({e}); left out of the raytracing scene", name);
            commands.entity(entity).insert(NotRaytraced);
            continue;
        }
        let extra: Vec<_> = mesh
            .attributes()
            .map(|(a, _)| a.id)
            .filter(|id| {
                ![
                    Mesh::ATTRIBUTE_POSITION.id,
                    Mesh::ATTRIBUTE_NORMAL.id,
                    Mesh::ATTRIBUTE_UV_0.id,
                    Mesh::ATTRIBUTE_TANGENT.id,
                ]
                .contains(id)
            })
            .collect();
        if !extra.is_empty() {
            warn!(
                "solari: {:?}: extra vertex attributes {extra:?}; left out of the raytracing scene",
                name
            );
            commands.entity(entity).insert(NotRaytraced);
            continue;
        }
        commands.entity(entity).insert(RaytracingMesh3d(mesh3d.0.clone()));
    }
}

/// A mesh Solari cannot take, so it is not looked at again.
#[derive(Component)]
struct NotRaytraced;

/// The main camera gets Solari; the haze camera is left alone (it is a
/// fog march, and Solari has no fog).
fn tag_cameras(
    mut commands: Commands,
    cameras: Query<Entity, (Added<Camera3d>, Without<HazeCamera>, Without<SolariLighting>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert((
            SolariLighting::default(),
            Msaa::Off,
            CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
        ));
    }
}

/// The emissive disc standing in for a spill in the raytracing scene.
#[derive(Component)]
struct AreaLight {
    material: Handle<StandardMaterial>,
}

/// Radius of the stand-in lens disc, metres. A 150 W beam's front lens
/// is about this; a par's is a little larger.
const LENS_RADIUS: f32 = 0.05;
/// The snoot tube is capped at this length: a 1.3° beam would want
/// two metres of tube, which would occlude everything around the head.
const MAX_SNOOT: f32 = 0.5;

/// One emissive disc per spill, as a child, facing the way the spot
/// points (-Z), a hair in front of the emitter's origin so it clears
/// the housing. Neither the disc nor its snoot has a `Mesh3d`: they
/// exist only for the rays.
fn spawn_area_lights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spills: Query<Entity, Added<FixtureSpill>>,
) {
    if spills.is_empty() {
        return;
    }
    let disc = meshes.add(
        Circle::new(LENS_RADIUS)
            .mesh()
            .build()
            .with_generated_tangents()
            .expect("a circle has uvs and normals"),
    );
    let snoot = meshes.add(
        Cylinder::new(LENS_RADIUS * 1.05, 1.0)
            .mesh()
            .without_caps()
            .build()
            .with_generated_tangents()
            .expect("a cylinder has uvs and normals"),
    );
    let black = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        perceptual_roughness: 1.0,
        ..default()
    });
    for spill in &spills {
        let material = materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::BLACK,
            ..default()
        });
        commands.entity(spill).with_children(|parent| {
            parent.spawn((
                AreaLight {
                    material: material.clone(),
                },
                RaytracingMesh3d(disc.clone()),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, 0.0, -0.02),
                Visibility::default(),
                Name::new("Solari area light"),
            ));
            parent.spawn((
                Snoot,
                RaytracingMesh3d(snoot.clone()),
                MeshMaterial3d(black.clone()),
                // Scaled per frame to the spill's cone; a cylinder's
                // axis is Y, the spot's is -Z.
                Transform::from_rotation(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(1.0, 0.01, 1.0)),
                Visibility::default(),
                Name::new("Solari snoot"),
            ));
        });
    }
}

#[derive(Component)]
struct Snoot;

/// Each frame the disc emits what the spill throws. Bevy's spot
/// `intensity` is candela × 4π (see `spawn::spot_lumens`); a Lambertian
/// disc of area A with radiance L has on-axis intensity L·A, so
/// L = cd / A. The snoot's length is what makes the disc's flood fall
/// off at the spill's outer angle: tan(outer) = r / h.
fn drive_area_lights(
    mut materials: ResMut<Assets<StandardMaterial>>,
    spills: Query<(&SpotLight, &Visibility, &Children), With<FixtureSpill>>,
    lights: Query<&AreaLight>,
    mut snoots: Query<&mut Transform, With<Snoot>>,
) {
    let area = core::f32::consts::PI * LENS_RADIUS * LENS_RADIUS;
    // Spike dial: a gain on every stand-in light, to find where the
    // picture sits without a rebuild per guess.
    let gain: f32 = std::env::var("IGNITION_SOLARI_GAIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    for (spot, visibility, children) in &spills {
        let on = *visibility != Visibility::Hidden && spot.intensity > 0.0;
        let radiance = if on {
            spot.intensity / (4.0 * core::f32::consts::PI) / area * gain
        } else {
            0.0
        };
        let color = spot.color.to_linear();
        for child in children.iter() {
            if let Ok(light) = lights.get(child)
                && let Some(mut material) = materials.get_mut(&light.material)
            {
                material.emissive =
                    LinearRgba::rgb(color.red * radiance, color.green * radiance, color.blue * radiance);
            }
            if let Ok(mut transform) = snoots.get_mut(child) {
                let h = (LENS_RADIUS * 1.05 / spot.outer_angle.tan().max(1e-3)).min(MAX_SNOOT);
                transform.scale.y = h.max(0.01);
                // The tube runs from the disc outward along -Z.
                transform.translation.z = -0.02 - h * 0.5;
            }
        }
    }
}
