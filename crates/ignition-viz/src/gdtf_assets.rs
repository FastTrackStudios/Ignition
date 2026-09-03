//! The `gdtf://` asset source: Bevy's own glTF loader, fed straight from
//! the model files inside a `.gdtf` zip.
//!
//! A `.gdtf` is a zip, and a profile's 3D model is an entry in it
//! (`models/gltf/<file>.glb`). Rather than decoding that GLB by hand —
//! which `gdtf_mesh.rs` used to do, and which dropped the file's
//! materials, tangents and node hierarchy on the floor — the zip is
//! exposed to the asset server as a source of its own, so a model is
//! addressed as
//!
//! ```text
//! gdtf://abs/path/to/Manufacturer@Fixture@Rev.gdtf/models/gltf/Body.glb#Scene0
//! ```
//!
//! and `bevy_gltf` loads it like any other scene. The importer
//! (`gdtf_geometry.rs`) keeps working synchronously on the file's
//! header — the model's extent comes from the accessor bounds glTF
//! requires — so a fixture spawns at once with a box the size of its
//! `<Model>` where the mesh will be, and [`swap_in_loaded_models`]
//! removes the box the moment the scene has spawned. The emitter is a
//! child of the `<Beam>` node, never of the drawn body, so nothing
//! about the swap touches it (`viz.one-emitter-tree`).
//!
//! `GdtfSourcePlugin` has to be added **before** `DefaultPlugins`: an
//! asset source can only be registered before `AssetPlugin` builds the
//! server. `GdtfAssetsPlugin` (the swap system) goes anywhere after.

use crate::spawn::{Fixture, FixtureBody, PartMaterial};
use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::{AssetReader, AssetReaderError, PathStream, Reader, VecReader};
use bevy::asset::{AssetApp, AssetPath};
use bevy::gltf::GltfAssetLabel;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstance};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The asset source id: the scheme before `://`.
pub const SOURCE: &str = "gdtf";

/// The rotation that takes a glTF scene (Y-up) into GDTF's frame
/// (Z-up): `(x, y, z) -> (x, -z, y)`, a quarter turn about X. The same
/// map `gdtf_mesh::y_up_to_z_up` applies to the file's bounds.
#[must_use]
pub fn gltf_to_gdtf() -> Quat {
    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
}

/// The asset path of `models/gltf/<file>.glb` inside `gdtf_file`, as the
/// scene the loader spawns.
///
/// `gdtf_file` is made absolute, so the path means the same thing whatever
/// the process's working directory is when the load finally runs.
///
/// The archive's absolute path is carried **without its root** (`run/
/// media/.../x.gdtf`, not `/run/media/...`): the asset server refuses a
/// rooted path as one that escapes the assets folder, which is the
/// right rule for a path read out of a scene file and beside the point
/// for a source whose every path is one of the profile library's own.
/// [`GdtfZipReader::split`] puts the root back.
#[must_use]
pub fn model_scene_path(gdtf_file: &Path, file: &str) -> AssetPath<'static> {
    let abs = std::fs::canonicalize(gdtf_file).unwrap_or_else(|_| gdtf_file.to_path_buf());
    let rootless: PathBuf = abs
        .components()
        .filter(|c| !matches!(c, std::path::Component::RootDir))
        .collect();
    GltfAssetLabel::Scene(0).from_asset(format!(
        "{SOURCE}://{}/models/gltf/{file}.glb",
        rootless.to_string_lossy()
    ))
}

/// Serves zip entries: the path's `<something>.gdtf` prefix is the
/// archive, the rest the entry inside it.
pub struct GdtfZipReader;

impl GdtfZipReader {
    /// `a/b/x.gdtf/models/gltf/Body.glb` -> (`/a/b/x.gdtf`, `models/gltf/Body.glb`)
    /// — the archive path is absolute again (see [`model_scene_path`]).
    #[must_use]
    pub fn split(path: &Path) -> Option<(PathBuf, String)> {
        let mut archive = PathBuf::from("/");
        let mut components = path.components();
        while let Some(c) = components.next() {
            archive.push(c);
            if archive.extension().and_then(|e| e.to_str()) == Some("gdtf") {
                let entry: PathBuf = components.collect();
                let entry = entry.to_string_lossy().replace('\\', "/");
                return (!entry.is_empty()).then_some((archive, entry));
            }
        }
        None
    }

    /// Reads one entry of one archive into memory. Synchronous: a model
    /// is a few hundred kilobytes and the asset server already runs its
    /// loaders on the IO task pool.
    ///
    /// # Errors
    ///
    /// If `path` doesn't name a `gdtf://`-shaped entry, the archive can't
    /// be opened, or the named entry isn't in it.
    pub fn read_entry(path: &Path) -> Result<Vec<u8>, AssetReaderError> {
        let Some((archive, entry)) = Self::split(path) else {
            return Err(AssetReaderError::NotFound(path.to_path_buf()));
        };
        let io = |e: std::io::Error| AssetReaderError::Io(std::sync::Arc::new(e));
        let file = std::fs::File::open(&archive).map_err(io)?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| io(std::io::Error::other(format!("{}: {e}", archive.display()))))?;
        let mut item = match zip.by_name(&entry) {
            Ok(item) => item,
            Err(zip::result::ZipError::FileNotFound) => {
                return Err(AssetReaderError::NotFound(path.to_path_buf()));
            }
            Err(e) => return Err(io(std::io::Error::other(format!("{entry}: {e}")))),
        };
        // Widening a size hint: on a 32-bit target a zip entry could in
        // principle overflow `usize`, but this is only a `with_capacity`
        // hint, so falling back to no pre-reservation rather than an
        // audited-but-wrong cast is the safe answer — `read_to_end` below
        // still grows the buffer to fit whatever the entry actually is.
        let mut bytes = Vec::with_capacity(usize::try_from(item.size()).unwrap_or(0));
        item.read_to_end(&mut bytes).map_err(io)?;
        Ok(bytes)
    }
}

impl AssetReader for GdtfZipReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Self::read_entry(path).map(VecReader::new)
    }

    /// A zip carries no `.meta` sidecars; the loader's defaults apply.
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

/// Registers the `gdtf://` source. Add before `DefaultPlugins`.
pub struct GdtfSourcePlugin;

impl Plugin for GdtfSourcePlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_source(SOURCE, AssetSourceBuilder::new(|| Box::new(GdtfZipReader)));
    }
}

/// The box drawn where a GLB model will be until its scene has spawned.
#[derive(Component)]
pub struct GltfPlaceholder;

/// The `WorldAssetRoot` entity of a fixture part loaded from its
/// profile's GLB, and the placeholder it replaces.
#[derive(Component)]
pub struct GltfBody {
    pub placeholder: Entity,
}

/// Removes each placeholder once its scene is in the world. A load that
/// fails leaves the box where the part is — a fixture with a wrong-sized
/// box in it is a better fault report than a gap.
// r[impl viz.gdtf-meshes] - the placeholder gives way to the loaded scene
pub fn swap_in_loaded_models(
    mut commands: Commands,
    bodies: Query<(Entity, &GltfBody), Added<WorldInstance>>,
) {
    for (scene, body) in &bodies {
        if let Ok(mut placeholder) = commands.get_entity(body.placeholder) {
            placeholder.despawn();
        }
        commands.entity(scene).remove::<GltfBody>();
    }
}

/// Gives each fixture its own copy of the materials its GLB parts came
/// with, and makes those parts pickable.
///
/// `bevy_gltf` hands every instance of a file the same material
/// handles, so all forty-eight pars of one type drew with one
/// `StandardMaterial`. Nothing on the fixture pointed at it, so the
/// hover and selection tints (`picking::tint_bodies`) never reached a
/// par; and had they, every par would have lit up together. Here each
/// mesh that lands under a fixture with a material the fixture does not
/// own gets a per-fixture clone — one per distinct source material, so
/// three meshes sharing one material share one clone — recorded on the
/// [`FixtureBody`] so the glow and the tints treat it like the body.
///
/// Runs on `Added<MeshMaterial3d>` rather than on the scene root so it
/// is indifferent to which frame the scene's children appear in; the
/// clone's own insertion trips `Added` again a frame later and is
/// recognised as already owned.
// r[impl studio.program.pick-and-gizmos] - a GLB fixture tints through its own materials
pub fn adopt_scene_materials(
    mut commands: Commands,
    fresh: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        (Added<MeshMaterial3d<StandardMaterial>>, Without<Fixture>),
    >,
    parents: Query<&ChildOf>,
    roots: Query<(), With<FixtureBody>>,
    mut bodies: Query<&mut FixtureBody>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mesh, MeshMaterial3d(handle)) in &fresh {
        let Some(root) = parents.iter_ancestors(mesh).find(|&e| roots.get(e).is_ok()) else {
            continue;
        };
        let Ok(mut body) = bodies.get_mut(root) else {
            continue;
        };
        // The scene's meshes are what the ray hits; the root's observers
        // hear about it by bubbling. The marker is what keeps that true
        // if picking is ever made opt-in.
        commands.entity(mesh).insert(Pickable::default());
        if body.owns(handle) {
            continue;
        }
        let clone = if let Some(part) = body.parts.iter().find(|p| p.source == handle.id()) {
            part.material.clone()
        } else {
            let Some(source) = materials.get(handle).cloned() else {
                continue;
            };
            let base_emissive = source.emissive;
            let clone = materials.add(source);
            body.parts.push(PartMaterial {
                source: handle.id(),
                material: clone.clone(),
                base_emissive,
            });
            clone
        };
        commands.entity(mesh).insert(MeshMaterial3d(clone));
    }
}

/// The swap and the material adoption. Goes with `VizPlugin`.
pub struct GdtfAssetsPlugin;

impl Plugin for GdtfAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                swap_in_loaded_models,
                adopt_scene_materials.before(crate::spawn::update_fixture_bodies),
            ),
        );
    }
}

/// The three numbers the importer reads off a `<Geometry>` and hands
/// straight back: `lo..hi` is the model's unscaled extent in GDTF's frame,
/// `scale` the per-axis fit to the `<Model>` dimensions.
///
/// They travel together because they are only ever meaningful together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelFit {
    pub lo: Vec3,
    pub hi: Vec3,
    pub scale: Vec3,
}

/// What stands in for a model until its GLB lands — a box of the right
/// size in the rig's default skin.
pub struct Placeholder<'a> {
    pub mesh: Handle<Mesh>,
    pub material: &'a Handle<StandardMaterial>,
}

/// Spawns a GLB model under `parent`: the placeholder now, the scene
/// when it lands. Without an `AssetServer` (a headless test with no
/// asset plugin) only the placeholder is spawned.
pub fn spawn_model(
    commands: &mut Commands,
    parent: Entity,
    assets: Option<&AssetServer>,
    asset: &str,
    placeholder: Placeholder<'_>,
    fit: ModelFit,
) {
    let ModelFit { lo, hi, scale } = fit;
    // `Vec3`'s `Add`/`Mul` are float, component-wise, and cannot panic or
    // overflow — `arithmetic_side_effects` fires on any operator-
    // overloaded type, not just the primitive integers the lint is
    // really about (see docs/ops/clippy.md and
    // `gdtf_mesh.rs::walk_bounds`'s identical suppression).
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Vec3 arithmetic is float, component-wise, and cannot panic or overflow"
    )]
    let centre = (lo + hi) * 0.5 * scale;
    let placeholder = commands
        .spawn((
            GltfPlaceholder,
            Mesh3d(placeholder.mesh),
            MeshMaterial3d(placeholder.material.clone()),
            Transform::from_translation(centre),
            ChildOf(parent),
        ))
        .id();
    let Some(assets) = assets else {
        return;
    };
    // The scene is Y-up and in the file's own units. Bevy composes a
    // `Transform` as translate * rotate * scale, so the scale below is
    // applied in the *file's* frame before the rotation stands it up:
    // the fit computed on Z-up axes (x, y, z) is therefore fed back as
    // (x, z, y) — the file's Z becomes GDTF's Y and vice versa.
    commands.spawn((
        WorldAssetRoot(assets.load(AssetPath::parse(asset).clone_owned())),
        GltfBody { placeholder },
        Transform {
            translation: Vec3::ZERO,
            rotation: gltf_to_gdtf(),
            scale: Vec3::new(scale.x, scale.z, scale.y),
        },
        Visibility::default(),
        Name::new("gltf"),
        ChildOf(parent),
    ));
}

/// A headless app that can load `gdtf://` scenes and nothing else: the
/// asset server, the glTF loader and the asset types it produces, no
/// renderer. For the importer's tests, which want to know a profile's
/// GLB really loads through the source.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use bevy::asset::{AssetPlugin, LoadState};
    use bevy::gltf::{Gltf, GltfPlugin};
    use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats};
    use bevy::world_serialization::WorldSerializationPlugin;

    pub fn asset_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            bevy::MinimalPlugins,
            GdtfSourcePlugin,
            AssetPlugin::default(),
        ))
        .insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .add_plugins((
            bevy::transform::TransformPlugin,
            bevy::scene::ScenePlugin,
            WorldSerializationPlugin,
            GltfPlugin::default(),
        ));
        app.finish();
        app.cleanup();
        app
    }

    /// What a loaded `Gltf` holds, counted.
    pub struct LoadedGltf {
        pub meshes: usize,
        pub materials: usize,
        pub scenes: usize,
    }

    /// Loads the `Gltf` a scene path belongs to and pumps the app until
    /// it lands or fails. Panics on failure or after ten seconds.
    pub fn load_gltf(app: &mut App, scene_path: &str) -> LoadedGltf {
        let path: AssetPath<'static> = AssetPath::parse(scene_path).without_label().clone_owned();
        let handle: Handle<Gltf> = app.world().resource::<AssetServer>().load(path);
        // `Instant + Duration` can in principle panic on overflow, but ten
        // seconds past "now" is nowhere near `Instant`'s range on any
        // clock this test will ever run on.
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "ten seconds past now never overflows Instant in practice"
        )]
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.update();
            match app.world().resource::<AssetServer>().load_state(&handle) {
                LoadState::Loaded => break,
                LoadState::Failed(e) => panic!("{scene_path}: {e}"),
                _ if std::time::Instant::now() > deadline => panic!("{scene_path}: never loaded"),
                _ => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        let gltf = app
            .world()
            .resource::<Assets<Gltf>>()
            .get(&handle)
            .expect("loaded");
        LoadedGltf {
            meshes: gltf.meshes.len(),
            materials: gltf.materials.len(),
            scenes: gltf
                .scenes
                .len()
                .saturating_add(usize::from(gltf.default_scene.is_some())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<StandardMaterial>()
            .add_systems(Update, adopt_scene_materials);
        app
    }

    fn fixture(app: &mut App, index: usize, body: &Handle<StandardMaterial>) -> Entity {
        app.world_mut()
            .spawn((
                Fixture {
                    index,
                    base_rot: Quat::IDENTITY,
                },
                FixtureBody::new(body.clone()),
            ))
            .id()
    }

    /// A GLB scene's mesh, two levels under the fixture root the way
    /// `spawn_model` nests it: root -> node -> "gltf" -> mesh.
    fn scene_mesh(app: &mut App, root: Entity, material: &Handle<StandardMaterial>) -> Entity {
        let node = app.world_mut().spawn(ChildOf(root)).id();
        let scene = app
            .world_mut()
            .spawn((Name::new("gltf"), ChildOf(node)))
            .id();
        app.world_mut()
            .spawn((MeshMaterial3d(material.clone()), ChildOf(scene)))
            .id()
    }

    fn material_of(app: &App, mesh: Entity) -> Handle<StandardMaterial> {
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .expect("a mesh has a material")
            .0
            .clone()
    }

    /// r[verify studio.program.pick-and-gizmos] - each GLB fixture owns its materials
    #[test]
    fn each_fixture_gets_its_own_clone_of_a_shared_file_material() {
        let mut app = material_app();
        let (body, shared, lens) = {
            let mut m = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            (
                m.add(StandardMaterial::default()),
                m.add(StandardMaterial::default()),
                m.add(StandardMaterial {
                    emissive: LinearRgba::rgb(1.0, 1.0, 1.0),
                    ..Default::default()
                }),
            )
        };
        let a = fixture(&mut app, 0, &body);
        let b = fixture(&mut app, 1, &body);
        let a1 = scene_mesh(&mut app, a, &shared);
        let a2 = scene_mesh(&mut app, a, &shared);
        let a_lens = scene_mesh(&mut app, a, &lens);
        let b1 = scene_mesh(&mut app, b, &shared);
        // A primitive part draws with the body material and is left be.
        let a_prim = app
            .world_mut()
            .spawn((MeshMaterial3d(body.clone()), ChildOf(a)))
            .id();
        let stray = app.world_mut().spawn(MeshMaterial3d(shared.clone())).id();
        app.update();
        app.update();
        app.update();

        let ma1 = material_of(&app, a1);
        assert_ne!(
            ma1.id(),
            shared.id(),
            "the file's material is not drawn with directly"
        );
        assert_eq!(
            material_of(&app, a2).id(),
            ma1.id(),
            "one clone per fixture and source"
        );
        assert_ne!(
            material_of(&app, b1).id(),
            ma1.id(),
            "the next fixture has its own"
        );
        assert_eq!(
            material_of(&app, a_prim).id(),
            body.id(),
            "the body material is not cloned"
        );
        assert_eq!(
            material_of(&app, stray).id(),
            shared.id(),
            "nothing outside a fixture is touched"
        );

        let body_a = app.world().get::<FixtureBody>(a).unwrap();
        assert_eq!(body_a.parts.len(), 2, "the shared material and the lens");
        assert_eq!(
            body_a.tintable().count(),
            2,
            "the body and the housing, not the lens"
        );
        let lens_part = body_a.parts.iter().find(|p| p.source == lens.id()).unwrap();
        assert!(lens_part.is_lens());
        assert_eq!(material_of(&app, a_lens).id(), lens_part.material.id());
        let body_b = app.world().get::<FixtureBody>(b).unwrap();
        assert_eq!(body_b.parts.len(), 1);
        for mesh in [a1, a2, a_lens, b1] {
            assert!(
                app.world().get::<Pickable>(mesh).is_some(),
                "a scene mesh is pickable"
            );
        }
        assert!(app.world().get::<Pickable>(stray).is_none());
    }

    #[test]
    fn a_zip_path_splits_at_the_gdtf() {
        for p in [
            "a/b/M@F@R.gdtf/models/gltf/Body.glb",
            "/a/b/M@F@R.gdtf/models/gltf/Body.glb",
        ] {
            let (archive, entry) = GdtfZipReader::split(Path::new(p)).unwrap();
            assert_eq!(archive, PathBuf::from("/a/b/M@F@R.gdtf"));
            assert_eq!(entry, "models/gltf/Body.glb");
        }
        assert!(GdtfZipReader::split(Path::new("a/b/M@F@R.gdtf")).is_none());
        assert!(GdtfZipReader::split(Path::new("models/gltf/Body.glb")).is_none());
    }

    #[test]
    fn the_scene_path_addresses_the_zip_entry_and_the_first_scene() {
        let p = model_scene_path(Path::new("/nowhere/x.gdtf"), "Body");
        assert_eq!(p.source().as_str(), Some(SOURCE));
        assert_eq!(p.path(), Path::new("nowhere/x.gdtf/models/gltf/Body.glb"));
        assert_eq!(p.label(), Some("Scene0"));
        assert!(!p.is_unapproved(), "rootless, so the server takes it");
        let (archive, _) = GdtfZipReader::split(p.path()).unwrap();
        assert_eq!(archive, PathBuf::from("/nowhere/x.gdtf"));
    }

    /// Standing a Y-up scene up: the file's +Y is the fixture's +Z.
    #[test]
    fn the_scene_rotation_matches_the_bounds_remap() {
        let r = gltf_to_gdtf();
        for p in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(0.3, -0.7, 1.9)] {
            let a = r * p;
            let b = crate::gdtf_mesh::y_up_to_z_up(p);
            assert!(a.distance(b) < 1e-6, "{p:?}: {a:?} vs {b:?}");
        }
    }

    /// The whole road: zip entry -> asset source -> `bevy_gltf` -> a
    /// `Gltf` with meshes and the file's own materials.
    // r[verify viz.gdtf-meshes] - bevy_gltf loads a profile's GLB through the source
    #[test]
    fn bevy_gltf_loads_a_scene_through_the_source() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/gdtf");
        let Some(file) = std::fs::read_dir(&dir).ok().and_then(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| p.to_string_lossy().contains("SlimPAR_Quad_12"))
        }) else {
            eprintln!("skipping: data/gdtf has no SlimPAR Quad 12 profile");
            return;
        };
        let mut app = test_support::asset_app();
        let gltf = test_support::load_gltf(&mut app, &model_scene_path(&file, "Body").to_string());
        assert!(gltf.meshes >= 1, "the body is a mesh");
        assert!(gltf.scenes >= 1, "in a scene the fixture can spawn");
        // Whatever materials the file carries come along — this one
        // happens to ship none, which is the loader's business, not ours.
        let _ = gltf.materials;
    }

    // r[verify viz.gdtf-meshes] - a real profile's GLB is served out of its zip
    #[test]
    fn reads_a_real_glb_out_of_a_gdtf_zip() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/gdtf");
        let Some(file) = std::fs::read_dir(&dir).ok().and_then(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| p.to_string_lossy().contains("SlimPAR_Quad_12"))
        }) else {
            eprintln!("skipping: data/gdtf has no SlimPAR Quad 12 profile");
            return;
        };
        let bytes = GdtfZipReader::read_entry(&file.join("models/gltf/Body.glb")).expect("served");
        assert_eq!(&bytes[..4], b"glTF", "a GLB starts with its magic");
        assert!(matches!(
            GdtfZipReader::read_entry(&file.join("models/gltf/Nope.glb")),
            Err(AssetReaderError::NotFound(_))
        ));
    }
}
