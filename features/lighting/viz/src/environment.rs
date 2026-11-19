//! The room as a model: `environment.json` beside a venue's other files.
//!
//! A venue's `room.json` is a handful of boxes, which is all a room
//! described over the phone can be. A room that has been measured gets
//! an environment instead: one glTF file holding the architecture and
//! the set dressing as separately named nodes, and this document, which
//! is the record of them — where each object stands, whether it is
//! shown, and the room's own practical lights in switchable groups.
//!
//! The document is the data and the model is only its shapes. Moving an
//! object, hiding it or switching a light group is an edit to
//! [`EnvironmentRes`]; [`apply_environment`] carries it onto the
//! entities. Nothing reads a position back out of the glTF.
//!
//! The model is found beside the document in `model/`, or in
//! `$IGNITION_ENVIRONMENT_DIR`. A venue whose model is not on this
//! machine draws its `room.json` boxes, as it always did.

use crate::venue::Vec3 as Point;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The document's name in a venue directory.
pub const FILE: &str = "environment.json";

/// The asset source the model is read through — see
/// [`EnvironmentSourcePlugin`].
pub const SOURCE: &str = "environment";

/// Where to look for a venue's model before `model/` beside it.
pub const DIR_VAR: &str = "IGNITION_ENVIRONMENT_DIR";

/// A venue's environment: the model's objects and the room's own lights.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    /// The glTF file, by name.
    pub model: String,
    #[serde(default)]
    pub light_groups: Vec<LightGroup>,
    #[serde(default)]
    pub objects: Vec<Object>,
    #[serde(default)]
    pub lights: Vec<Light>,
    /// What this build does not read (the measurements, the materials'
    /// provenance), kept so a save writes it back.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
    /// The directory the model was found in. `None` when it is not on
    /// this machine, which leaves the venue on its `room.json` boxes.
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

/// A set of lights switched together: the house lights.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightGroup {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub on: bool,
}

/// One named node of the model. The same shape as a `room.json` record
/// (name, position, eulers, size) with what an editor needs beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub name: String,
    /// `architecture`, `furniture`, `gear`, `practical` or `found-light`.
    pub kind: String,
    /// Where its origin stands, in venue metres. An object's origin is on
    /// the floor under it.
    pub position: Point,
    /// Degrees; only `z`, the turn about the vertical, is applied.
    pub eulers: Point,
    /// Its extent before the turn. Not applied: the model is real scale.
    pub size: Point,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default = "yes")]
    pub movable: bool,
    /// Shown only while this light group is on: a lit lens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One practical light.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub name: String,
    pub group: String,
    /// `point` or `spot`.
    pub kind: String,
    pub position: Point,
    /// Luminous flux.
    pub lumens: f32,
    /// Linear RGB, largest channel 1.
    pub color: [f32; 3],
    pub range: f32,
    /// The source's size. A large one is a soft fill with no highlight.
    #[serde(default = "small_source")]
    pub radius: f32,
    #[serde(default)]
    pub shadows: bool,
    /// What a spot points at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aim: Option<Point>,
    /// A spot's full cone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cone_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

const fn yes() -> bool {
    true
}

const fn small_source() -> f32 {
    0.05
}

impl Environment {
    /// Parse the document. The model is not looked for: see [`Self::load`].
    ///
    /// # Errors
    ///
    /// If the document is not an environment.
    pub fn from_json(raw: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }

    /// The venue's environment, if it has one, with its model located.
    ///
    /// # Errors
    ///
    /// If the document is there and will not parse. A venue without one
    /// is `Ok(None)`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(venue_dir: &Path) -> anyhow::Result<Option<Self>> {
        let Ok(raw) = std::fs::read_to_string(venue_dir.join(FILE)) else {
            return Ok(None);
        };
        let mut environment = Self::from_json(&raw)
            .map_err(|e| anyhow::anyhow!("{}: {e}", venue_dir.join(FILE).display()))?;
        let candidates = std::env::var_os(DIR_VAR)
            .map(PathBuf::from)
            .into_iter()
            .chain([venue_dir.join("model")]);
        environment.dir = candidates
            .filter(|dir| dir.join(&environment.model).is_file())
            .find_map(|dir| dir.canonicalize().ok());
        if environment.dir.is_none() {
            tracing::warn!(
                model = environment.model,
                venue = %venue_dir.display(),
                "the venue's environment model is not here (model/ beside it, or \
                 ${DIR_VAR}); drawing its room.json boxes"
            );
        }
        Ok(Some(environment))
    }

    /// Whether the model is to hand, and so replaces the `room.json` boxes.
    #[must_use]
    pub const fn has_model(&self) -> bool {
        self.dir.is_some()
    }

    /// Whether a light group is on. A group nobody declared is off.
    #[must_use]
    pub fn group_on(&self, name: &str) -> bool {
        self.light_groups.iter().any(|g| g.name == name && g.on)
    }

    /// Switch a light group. `false` if the room has no such group.
    pub fn switch(&mut self, name: &str, on: bool) -> bool {
        let Some(group) = self.light_groups.iter_mut().find(|g| g.name == name) else {
            return false;
        };
        group.on = on;
        true
    }

    /// Whether an object is drawn: shown, and its light group (if it is a
    /// lit lens) on.
    #[must_use]
    pub fn shows(&self, object: &Object) -> bool {
        object.visible && object.group.as_deref().is_none_or(|g| self.group_on(g))
    }
}

/// Whether a `room.json` record is one the model draws instead.
#[must_use]
pub fn replaces(name: &str) -> bool {
    name == "Ceiling" || name.starts_with("Floor") || name.starts_with("Wall")
}

/// The environment being drawn. Edit it and the room follows.
#[derive(Resource, Debug, Clone, Default)]
pub struct EnvironmentRes(pub Environment);

/// The model's root entity.
#[derive(Component)]
pub struct EnvironmentRoot;

/// A practical light's entity, by its index in [`Environment::lights`].
#[derive(Component)]
pub struct PracticalLight(pub usize);

/// Registers the `environment://` source on a venue's model directory.
/// Add before `DefaultPlugins`, like `GdtfSourcePlugin`: a source has to
/// exist before the asset server is built.
pub struct EnvironmentSourcePlugin(pub Option<PathBuf>);

impl EnvironmentSourcePlugin {
    #[must_use]
    pub fn for_venue(venue: &crate::Venue) -> Self {
        Self(venue.environment.as_ref().and_then(|e| e.dir.clone()))
    }
}

impl Plugin for EnvironmentSourcePlugin {
    #[cfg(not(target_arch = "wasm32"))]
    fn build(&self, app: &mut App) {
        use bevy::asset::io::AssetSourceBuilder;
        use bevy::asset::io::file::FileAssetReader;
        let Some(dir) = self.0.clone() else { return };
        app.register_asset_source(
            SOURCE,
            AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(dir.clone()))),
        );
    }

    #[cfg(target_arch = "wasm32")]
    fn build(&self, _app: &mut App) {}
}

/// glTF is Y-up and the venue is Z-up: the model's root carries the turn,
/// and a record's venue pose goes onto its node in the model's own frame.
fn model_frame(position: Vec3, yaw_deg: f32) -> Transform {
    Transform {
        translation: Vec3::new(position.x, position.z, -position.y),
        rotation: Quat::from_rotation_y(yaw_deg.to_radians()),
        scale: Vec3::ONE,
    }
}

/// Spawns the model and the practical lights. Runs beside `spawn_venue`.
#[expect(
    clippy::needless_pass_by_value,
    clippy::arithmetic_side_effects,
    reason = "a Bevy system's Res parameters are taken by value, and the Vec3 \
              arithmetic is float and component-wise (docs/lighting/ops/clippy.md)"
)]
pub fn spawn_environment(
    mut commands: Commands,
    venue: Res<crate::spawn::VenueRes>,
    asset_server: Res<AssetServer>,
) {
    let Some(environment) = venue.0.environment.as_ref().filter(|e| e.has_model()) else {
        return;
    };
    let scene = GltfAssetLabel::Scene(0).from_asset(format!("{SOURCE}://{}", environment.model));
    commands.spawn((
        WorldAssetRoot(asset_server.load(scene)),
        Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        Name::new(format!("Environment - {}", environment.name)),
        EnvironmentRoot,
    ));
    for (index, light) in environment.lights.iter().enumerate() {
        let color = Color::linear_rgb(light.color[0], light.color[1], light.color[2]);
        let at = light.position.to_vec3();
        let mut entity = commands.spawn((
            Name::new(format!("Practical - {}", light.name)),
            PracticalLight(index),
            Visibility::Hidden,
        ));
        match light.aim {
            Some(aim) => {
                let half = light.cone_deg.unwrap_or(60.0).to_radians() / 2.0;
                let aim = aim.to_vec3();
                // Straight up or down has no "up" to look along but Z.
                let up = if (aim - at).normalize_or_zero().z.abs() > 0.99 {
                    Vec3::Y
                } else {
                    Vec3::Z
                };
                entity.insert((
                    SpotLight {
                        color,
                        intensity: light.lumens,
                        range: light.range,
                        radius: light.radius,
                        outer_angle: half,
                        inner_angle: half * 0.55,
                        shadow_maps_enabled: light.shadows,
                        ..default()
                    },
                    Transform::from_translation(at).looking_at(aim, up),
                ));
            }
            None => {
                entity.insert((
                    PointLight {
                        color,
                        intensity: light.lumens,
                        range: light.range,
                        radius: light.radius,
                        shadow_maps_enabled: light.shadows,
                        ..default()
                    },
                    Transform::from_translation(at),
                ));
            }
        }
    }
    commands.insert_resource(EnvironmentRes(environment.clone()));
}

/// Carries [`EnvironmentRes`] onto the entities: each object's pose and
/// whether it is drawn, each light's group.
///
/// Runs every frame until the model's nodes have all appeared (a glTF
/// scene spawns some frames after it is asked for), and after that only
/// when the resource changes.
pub fn apply_environment(
    environment: Option<Res<EnvironmentRes>>,
    roots: Query<Entity, With<EnvironmentRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    lights: Query<(Entity, &PracticalLight)>,
    mut settled: Local<bool>,
) {
    let Some(environment) = environment else {
        return;
    };
    if *settled && !environment.is_changed() {
        return;
    }
    let environment = &environment.0;
    for (entity, light) in &lights {
        let on = environment
            .lights
            .get(light.0)
            .is_some_and(|l| environment.group_on(&l.group));
        if let Ok(mut seen) = visibility.get_mut(entity) {
            *seen = if on {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    let mut found = 0usize;
    for root in &roots {
        for entity in children.iter_descendants(root) {
            let Ok(name) = names.get(entity) else {
                continue;
            };
            let Some(object) = environment.objects.iter().find(|o| o.name == name.as_str()) else {
                continue;
            };
            found = found.saturating_add(1);
            if let Ok(mut transform) = transforms.get_mut(entity) {
                *transform = model_frame(object.position.to_vec3(), object.eulers.z);
            }
            if let Ok(mut seen) = visibility.get_mut(entity) {
                *seen = if environment.shows(object) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
    *settled = found >= environment.objects.len();
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = r#"{
        "version": 1, "name": "Test", "model": "room.glb",
        "measured": {"width_m": 14.0},
        "light_groups": [{"name": "house", "label": "House lights", "on": false}],
        "objects": [
            {"name": "Piano", "kind": "gear", "position": {"x": 1, "y": 2, "z": 0},
             "eulers": {"x": 0, "y": 0, "z": 45}, "size": {"x": 1.5, "y": 1.9, "z": 1.6},
             "visible": true, "movable": true},
            {"name": "Lens", "kind": "practical", "position": {"x": 0, "y": 0, "z": 0},
             "eulers": {"x": 0, "y": 0, "z": 0}, "size": {"x": 1, "y": 1, "z": 1},
             "group": "house"}
        ],
        "lights": [{"name": "Can 1", "group": "house", "kind": "spot",
                    "position": {"x": 0, "y": 0, "z": 3.4}, "lumens": 900,
                    "color": [1, 0.8, 0.6], "range": 6,
                    "aim": {"x": 0, "y": 0, "z": 0}, "cone_deg": 60}]
    }"#;

    #[test]
    fn a_lit_lens_shows_with_its_group() {
        let mut environment = Environment::from_json(DOCUMENT).unwrap();
        let lens = environment.objects[1].clone();
        assert!(environment.shows(&environment.objects[0]));
        assert!(!environment.shows(&lens));
        assert!(environment.switch("house", true));
        assert!(environment.shows(&lens));
        assert!(!environment.switch("work lights", true));
    }

    #[test]
    fn what_is_not_read_is_written_back() {
        let environment = Environment::from_json(DOCUMENT).unwrap();
        let written = serde_json::to_value(&environment).unwrap();
        assert_eq!(written["measured"]["width_m"], 14.0);
        assert_eq!(written["version"], 1);
    }

    #[test]
    fn a_venue_pose_lands_in_the_models_frame() {
        // The root's quarter turn about X takes the model's Y to the
        // venue's Z; a node placed here must come back out where the
        // record said.
        let root = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let node = model_frame(Vec3::new(1.0, 2.0, 0.5), 90.0);
        assert!((root * node.translation).distance(Vec3::new(1.0, 2.0, 0.5)) < 1e-5);
        // A quarter turn about the vertical takes the object's +x to the venue's +y.
        let x = root * node.rotation * Vec3::X;
        assert!(x.distance(Vec3::Y) < 1e-5);
    }

    #[test]
    fn only_the_shell_gives_way_to_the_model() {
        assert!(replaces("Floor"));
        assert!(replaces("Wall - Upstage"));
        assert!(replaces("Ceiling"));
        assert!(!replaces("Column - Tower 1"));
    }
}
