//! The set-dressing that is not a light: people, mic stands and the kit.
//!
//! Three different answers to "what shape is this", picked per prop
//! rather than by habit:
//!
//! A **mic stand** is four primitives — a round base, a pole, a boom arm
//! and a capsule — so it is built here from Bevy's own shapes. That costs
//! nothing, carries no licence, and is parametric: boom angle and height
//! come from the venue record rather than from whatever a modeller
//! happened to export.
//!
//! A **person** and a **drum kit** are the opposite case. A human
//! silhouette and a five-piece kit are exactly what primitives cannot
//! fake — which is why the people in this venue were hidden for so long,
//! "a tall box with no readable silhouette, easy to mistake for a
//! fixture". Those are worth real models; see `assets/*/LICENSE-NOTICE`.

use crate::venue::GeometryRecord;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use std::f32::consts::PI;

/// The character meshes, relative to the asset root.
const MEN: [&str; 2] = ["people/man-casual.glb", "people/man-casual-2.glb"];

/// The kit, relative to the asset root.
const DRUM_KIT: &str = "props/drum-kit.glb";

/// Real height of a person, in metres — what the venue records measure
/// and what the models are scaled to match.
const MODEL_HEIGHT: f32 = 1.8;

/// glTF is Y-up; this project's world is Z-up (see `view.rs`). Rotating
/// on the way in keeps the downloaded files byte-identical to what their
/// authors published, which matters for being able to re-fetch them.
fn gltf_to_world() -> Quat {
    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
}

// ---------------------------------------------------------------------
// People
// ---------------------------------------------------------------------

/// How a spawned character should stand — or sit.
///
/// Quaternius' modular packs ship geometry and a rig but no animation
/// clips (their animations live in the separate Universal Animation
/// Library, which is built on a *different*, Rigify-style skeleton whose
/// bone names do not match, so its clips cannot be retargeted onto these
/// characters without a rebind). A character therefore spawns in its bind
/// pose, arms straight out, which reads as a mannequin — worse than the
/// box it replaced.
///
/// Rather than drag in a retargeting layer to play a single frame, the
/// two poses this venue actually needs are solved directly against the
/// skeleton below. A static visualizer needs one frame; if these ever
/// have to move, the right answer is the animation library plus a rebind,
/// not more entries in this enum.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pose {
    /// Arms down at the sides, weight even. Everyone on stage but the
    /// drummer.
    Standing,
    /// Perched on a throne, thighs forward, forearms out over the kit.
    SittingAtDrums,
}

/// Where each bone should end up *pointing*, in the character's own
/// frame: `+X` is the character's left, `-Y` is the way they are facing,
/// `+Z` is up. Bones are listed parents-first, because posing a parent
/// moves its children and the solver reads the current pose each time.
///
/// Aiming a bone rather than rotating it by a fixed angle is what makes
/// this robust: it needs no knowledge of the rig's bone roll or rest
/// orientation, only the fact — true of every Blender export — that a
/// bone points along its own local `+Y`.
type PoseTable = &'static [(&'static str, Vec3)];

const STANDING: PoseTable = &[
    ("UpperArm.L", Vec3::new(0.17, 0.03, -0.985)),
    ("UpperArm.R", Vec3::new(-0.17, 0.03, -0.985)),
    ("LowerArm.L", Vec3::new(0.11, -0.26, -0.96)),
    ("LowerArm.R", Vec3::new(-0.11, -0.26, -0.96)),
];

const SITTING_AT_DRUMS: PoseTable = &[
    ("UpperLeg.L", Vec3::new(0.13, -0.96, -0.25)),
    ("UpperLeg.R", Vec3::new(-0.13, -0.96, -0.25)),
    ("LowerLeg.L", Vec3::new(0.06, -0.16, -0.985)),
    ("LowerLeg.R", Vec3::new(-0.06, -0.16, -0.985)),
    ("Foot.L", Vec3::new(0.0, -0.93, 0.37)),
    ("Foot.R", Vec3::new(0.0, -0.93, 0.37)),
    ("UpperArm.L", Vec3::new(0.30, -0.34, -0.89)),
    ("UpperArm.R", Vec3::new(-0.30, -0.34, -0.89)),
    ("LowerArm.L", Vec3::new(0.10, -0.93, -0.35)),
    ("LowerArm.R", Vec3::new(-0.10, -0.93, -0.35)),
];

impl Pose {
    fn table(self) -> PoseTable {
        match self {
            Pose::Standing => STANDING,
            Pose::SittingAtDrums => SITTING_AT_DRUMS,
        }
    }

    /// How far the whole character sinks, as a fraction of their height.
    ///
    /// The pose bends the legs but cannot move the mesh's origin, which
    /// stays at the character's feet. Standing, hips sit about 0.53 of
    /// the way up; folded onto a throne the feet end up roughly a shin
    /// below the hips instead of a whole leg, so the body has to come
    /// down by the difference or the drummer floats above their stool.
    fn sink(self) -> f32 {
        match self {
            Pose::Standing => 0.0,
            Pose::SittingAtDrums => 0.27,
        }
    }
}

/// Spawns a person at a venue record's placement.
///
/// The record's `size.z` is the real measured height, so the model is
/// scaled to match it rather than trusted to be the right size — the
/// venue has people from 1.82 to 1.83 m and a model that ignored that
/// would look uniform in a way a real stage never does.
pub fn spawn_person(
    commands: &mut Commands,
    asset_server: &AssetServer,
    record: &GeometryRecord,
    index: usize,
    pose: Pose,
) {
    let height = record.size.z;
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(MEN[index % MEN.len()]));
    commands.spawn((
        WorldAssetRoot(scene),
        Transform {
            translation: record.position.to_vec3() - Vec3::Z * (pose.sink() * height),
            rotation: record.orientation() * gltf_to_world(),
            scale: Vec3::splat(height / MODEL_HEIGHT),
        },
        Name::new(record.name.clone()),
        pose,
        NeedsPose,
    ));
}

/// Marks a spawned character whose skeleton has not been posed yet.
#[derive(Component)]
pub struct NeedsPose;

/// Aims every bone in a freshly-spawned character's pose table.
///
/// glTF scenes load asynchronously, so this runs every frame and clears
/// the marker only once the skeleton is actually there to pose.
pub fn pose_new_characters(
    mut commands: Commands,
    pending: Query<(Entity, &Pose), With<NeedsPose>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    mut transforms: Query<&mut Transform>,
) {
    for (root, pose) in &pending {
        // The root carries the venue record's yaw composed with the
        // Y-up→Z-up fix; undoing the latter recovers the character's own
        // frame, which is what the pose tables are written in.
        let Ok(root_transform) = transforms.get(root) else {
            continue;
        };
        let facing = root_transform.rotation * gltf_to_world().inverse();

        let mut posed = 0usize;
        for entity in children.iter_descendants(root) {
            let Ok(name) = names.get(entity) else {
                continue;
            };
            let Some((_, target)) = pose.table().iter().find(|(b, _)| *b == name.as_str()) else {
                continue;
            };
            if aim_bone(entity, root, facing * *target, &parents, &mut transforms) {
                posed += 1;
            }
        }
        if posed == pose.table().len() {
            commands.entity(root).remove::<NeedsPose>();
        }
    }
}

/// Rotates one bone so it points along `target` (a world-space
/// direction), leaving its parents alone.
///
/// The bone's world rotation is rebuilt by walking up to `root` rather
/// than read from `GlobalTransform`, so this does not depend on where in
/// the schedule transform propagation last ran — the pose is correct on
/// the very first frame the skeleton exists.
fn aim_bone(
    bone: Entity,
    root: Entity,
    target: Vec3,
    parents: &Query<&ChildOf>,
    transforms: &mut Query<&mut Transform>,
) -> bool {
    let Some(parent) = parents.get(bone).ok().map(|p| p.parent()) else {
        return false;
    };
    let Some(parent_world) = world_rotation(parent, root, parents, transforms) else {
        return false;
    };
    let Ok(mut local) = transforms.get_mut(bone) else {
        return false;
    };
    let world = parent_world * local.rotation;
    // Every Blender bone points along its own local +Y.
    let arc = Quat::from_rotation_arc((world * Vec3::Y).normalize(), target.normalize());
    local.rotation = parent_world.inverse() * arc * world;
    true
}

/// Accumulated rotation from `root` down to `entity`, inclusive of both.
fn world_rotation(
    entity: Entity,
    root: Entity,
    parents: &Query<&ChildOf>,
    transforms: &Query<&mut Transform>,
) -> Option<Quat> {
    let mut chain = vec![entity];
    while *chain.last()? != root {
        chain.push(parents.get(*chain.last()?).ok()?.parent());
    }
    let mut rotation = Quat::IDENTITY;
    for entity in chain.iter().rev() {
        rotation *= transforms.get(*entity).ok()?.rotation;
    }
    Some(rotation)
}

// ---------------------------------------------------------------------
// Drum kit
// ---------------------------------------------------------------------

/// Height of the model as authored, in its own units — cymbal tops
/// included, which is what the venue record's `size.z` measures too.
const KIT_MODEL_HEIGHT: f32 = 3.191;

/// Centre of the model's footprint in its own (Y-up) coordinates. The
/// kit was not authored around its origin, so this recentres it on the
/// venue record instead of letting it drift a metre downstage.
const KIT_MODEL_CENTRE: Vec3 = Vec3::new(0.766, 0.0, 0.147);

/// Spawns the kit, facing the audience.
///
/// The model is authored facing its own `-Z`, which the Y-up→Z-up fix
/// turns into *upstage*; the extra half-turn is what points the kick at
/// the room rather than at the back wall.
pub fn spawn_drum_kit(
    commands: &mut Commands,
    asset_server: &AssetServer,
    record: &GeometryRecord,
) {
    let scale = record.size.z / KIT_MODEL_HEIGHT;
    let rotation = record.orientation() * Quat::from_rotation_z(PI) * gltf_to_world();
    commands.spawn((
        WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(DRUM_KIT))),
        Transform {
            translation: record.position.to_vec3() - rotation * (KIT_MODEL_CENTRE * scale),
            rotation,
            scale: Vec3::splat(scale),
        },
        Name::new(record.name.clone()),
    ));
}

// ---------------------------------------------------------------------
// Mic stands
// ---------------------------------------------------------------------

/// A microphone stand: round base, pole, boom, capsule.
///
/// `record.size.z` is the stand's real height and `size.y` its footprint
/// depth — a straight stand takes up almost none, a boom stand takes up
/// the reach of its arm, which is what distinguishes the two in the
/// venue data without anyone having to label them.
pub fn spawn_mic_stand(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    record: &GeometryRecord,
) {
    let height = record.size.z.max(0.9);
    let boom_reach = record.size.y - 0.55;
    let has_boom = boom_reach > 0.15;

    let metal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.07, 0.08),
        perceptual_roughness: 0.45,
        metallic: 0.7,
        ..default()
    });

    let root = commands
        .spawn((
            Transform {
                translation: record.position.to_vec3(),
                rotation: record.orientation(),
                scale: Vec3::ONE,
            },
            Visibility::default(),
            Name::new(record.name.clone()),
        ))
        .id();

    // Bevy's cylinders and capsules stand on +Y; this world is Z-up.
    let upright = Quat::from_rotation_arc(Vec3::Y, Vec3::Z);

    // Tripod base.
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.18, 0.03))),
        MeshMaterial3d(metal.clone()),
        Transform {
            translation: Vec3::Z * 0.015,
            rotation: upright,
            ..default()
        },
        ChildOf(root),
    ));
    // Pole.
    let pole = height - 0.03;
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.017, pole))),
        MeshMaterial3d(metal.clone()),
        Transform {
            translation: Vec3::Z * (0.03 + pole * 0.5),
            rotation: upright,
            ..default()
        },
        ChildOf(root),
    ));

    // A boom stand's arm angles down and forward from the top of the
    // pole; a straight stand just puts the mic on top of it.
    let (mic_at, mic_rot) = if has_boom {
        let reach = boom_reach.min(0.85);
        let drop = reach * 0.30;
        let tip = Vec3::new(0.0, -reach, height - drop);
        let along = (tip - Vec3::Z * height).normalize();
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(0.013, tip.distance(Vec3::Z * height)))),
            MeshMaterial3d(metal.clone()),
            Transform {
                translation: (Vec3::Z * height + tip) * 0.5,
                rotation: Quat::from_rotation_arc(Vec3::Y, along),
                ..default()
            },
            ChildOf(root),
        ));
        (tip, Quat::from_rotation_arc(Vec3::Y, -along))
    } else {
        (
            Vec3::Z * height,
            Quat::from_rotation_arc(Vec3::Y, Vec3::new(0.0, -0.6, -0.8).normalize()),
        )
    };

    // The capsule itself.
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.023, 0.075))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.14, 0.14, 0.15),
            perceptual_roughness: 0.35,
            metallic: 0.85,
            ..default()
        })),
        Transform {
            translation: mic_at,
            rotation: mic_rot,
            ..default()
        },
        ChildOf(root),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pose tables are read as directions; a zero or denormalised
    /// entry would silently aim a limb at nothing.
    #[test]
    fn pose_targets_are_directions() {
        for pose in [Pose::Standing, Pose::SittingAtDrums] {
            for (bone, target) in pose.table() {
                assert!(target.length() > 0.5, "{bone} target is degenerate");
            }
        }
    }

    /// Left and right are mirror images about the character's centre
    /// plane — a sign slip here reads instantly as a broken arm.
    #[test]
    fn pose_tables_are_symmetric() {
        for pose in [Pose::Standing, Pose::SittingAtDrums] {
            for (bone, target) in pose.table() {
                let Some(stem) = bone.strip_suffix(".L") else {
                    continue;
                };
                let right = format!("{stem}.R");
                let (_, mirrored) = pose
                    .table()
                    .iter()
                    .find(|(b, _)| **b == right)
                    .unwrap_or_else(|| panic!("{bone} has no {right}"));
                assert_eq!(*mirrored, Vec3::new(-target.x, target.y, target.z));
            }
        }
    }
}
