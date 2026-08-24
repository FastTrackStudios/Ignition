//! Maps a patched fixture's manufacturer/model string (straight from the
//! live Eos patch — see `docs/domain/norco-patch-and-groups.md`) to a real
//! shape and real dimensions, instead of the one-size box+cone every
//! fixture used to render as regardless of whether it's a 12cm par or a
//! 1m LED bar.
//!
//! Where a real fixture shape exists, it's one of QLC+'s generic
//! per-category meshes (par/moving-head/hazer — see
//! `assets/qlc-meshes/LICENSE-NOTICE.txt`), scaled to the closest
//! real-world dimension found for the actual fixture (manufacturer
//! listings, Open Fixture Library — see the commit that introduced this
//! file for sources). Not exact to the millimetre, but a real par can no
//! longer looks the same size and shape as a real bar or a real hazer.

use crate::mesh::MeshBuilder;
use crate::obj_mesh::ObjMesh;
use glam::{Quat, Vec3};
use std::sync::OnceLock;

macro_rules! qlc_mesh {
    ($name:ident, $file:literal) => {
        fn $name() -> &'static ObjMesh {
            static CELL: OnceLock<ObjMesh> = OnceLock::new();
            CELL.get_or_init(|| ObjMesh::parse(include_str!(concat!("../assets/qlc-meshes/", $file))))
        }
    };
}

qlc_mesh!(par_mesh, "par.obj");
qlc_mesh!(moving_head_mesh, "moving_head.obj");
qlc_mesh!(hazer_mesh, "hazer.obj");
qlc_mesh!(scanner_mesh, "scanner.obj");
qlc_mesh!(smoke_mesh, "smoke.obj");
qlc_mesh!(strobe_mesh, "strobe.obj");

/// Uniform scale that brings `mesh`'s bounding box's largest dimension to
/// `target_size` metres.
fn scale_to(mesh: &ObjMesh, target_size: f32) -> f32 {
    target_size / (mesh.max_half_extent() * 2.0)
}

/// The converted `moving_head.obj` sits at identity rotation in its own
/// "resting upright" pose (yoke base down, head up) — but every placement
/// quaternion in this project (Augment3d's own convention, per
/// `norco-venue-reference.md`) treats identity as "hung from the truss,"
/// the opposite pose. Confirmed backwards 2026-08-24: with no correction,
/// the truss-hung centre OH movers rendered upright and the floor-standing
/// units rendered hanging — exactly inverted. One 180° pre-rotation on the
/// mesh (not per-fixture data) fixes every moving-head instance at once,
/// since it's a mesh-convention mismatch, not a placement error.
fn moving_head_pre_rotate() -> Quat {
    Quat::from_rotation_x(std::f32::consts::PI)
}

pub enum Shape {
    /// One of QLC+'s generic category meshes, scaled to a real fixture's
    /// approximate overall size. `pre_rotate` is applied before the
    /// fixture's own placement rotation — see `moving_head_pre_rotate`.
    Mesh { mesh: &'static ObjMesh, target_size: f32, pre_rotate: Quat },
    /// An LED bar/batten: one elongated box, no beam (washes along its
    /// length rather than from a point) — no QLC+ mesh fits this shape.
    Bar { length: f32, width: f32, height: f32 },
    /// No dedicated profile — falls back to the generic marker.
    Generic,
}

/// `manufacturer`/`model` come verbatim from the live Eos patch
/// (`fixtures.json`'s `manufacturer`/`model` fields). Matched by substring
/// since Eos model strings vary in exact punctuation between channels of
/// the same physical fixture type (e.g. different DMX-mode suffixes).
pub fn shape_for(manufacturer: &str, model: &str) -> Shape {
    let m = manufacturer.to_ascii_lowercase();
    let mo = model.to_ascii_lowercase();

    if m == "uking" && mo.contains("par") {
        // Open Fixture Library's U`King Par Light B262: 180x180x100mm —
        // largest real dimension ~0.18m.
        return Shape::Mesh { mesh: par_mesh(), target_size: 0.20, pre_rotate: Quat::IDENTITY };
    }
    if m == "betopper" {
        // No published dimensions for the LB150; sized from the class of
        // compact 150W beam movers it belongs to.
        return Shape::Mesh {
            mesh: moving_head_mesh(),
            target_size: 0.35,
            pre_rotate: moving_head_pre_rotate(),
        };
    }
    if (m == "riukoe" || m == "lixada") && mo.contains("gobo") {
        // Lixada's own listing for the same 11ch shell: 14.5 x 17 x 23.5cm.
        return Shape::Mesh {
            mesh: moving_head_mesh(),
            target_size: 0.235,
            pre_rotate: moving_head_pre_rotate(),
        };
    }
    if m == "rockville" && mo.contains("rockstrip") {
        // Rockville's spec: 40.16 x 2.64 x 2.56 in.
        return Shape::Bar { length: 1.02, width: 0.067, height: 0.065 };
    }
    if m == "chauvet" && mo.contains("hurricane") {
        // Chauvet's spec for the Hurricane Haze 1DX: 11 x 6 x 9 in —
        // largest real dimension ~0.28m.
        return Shape::Mesh { mesh: hazer_mesh(), target_size: 0.28, pre_rotate: Quat::IDENTITY };
    }
    Shape::Generic
}

/// Not matched by any patched Norco fixture today, but available for the
/// next venue: scanner/smoke/strobe meshes at a generic ~0.3m size.
#[allow(dead_code)]
pub fn generic_shape(kind: &str) -> Shape {
    let mesh = match kind {
        "scanner" => scanner_mesh(),
        "smoke" => smoke_mesh(),
        "strobe" => strobe_mesh(),
        _ => return Shape::Generic,
    };
    Shape::Mesh { mesh, target_size: 0.30, pre_rotate: Quat::IDENTITY }
}

pub fn add_typed_fixture(mesh: &mut MeshBuilder, pos: Vec3, rot: Quat, manufacturer: &str, model: &str, color: [f32; 3]) {
    match shape_for(manufacturer, model) {
        Shape::Mesh { mesh: asset, target_size, pre_rotate } => {
            let scale = scale_to(asset, target_size);
            mesh.add_mesh_asset(pos, rot * pre_rotate, scale, asset, color);
        }
        Shape::Bar { length, width, height } => mesh.add_bar(pos, rot, length, width, height, color),
        Shape::Generic => mesh.add_fixture(pos, rot, color),
    }
}
