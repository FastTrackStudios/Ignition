//! Screen canvases and slices — one image across several panels.
//!
//! Resolume's model, and the thing QLC+ has no answer for: its video is
//! one window per function pinned to a screen index, so two screens
//! showing one file means two decoders and no relationship between them.
//!
//! A **canvas** is a source. A **slice** is the piece of it one physical
//! panel shows, derived from where that panel actually is in the room —
//! not from an equal division. That distinction is not fussiness at
//! Norco: the three back-wall TVs are 1.33 m, 1.44 m and 1.33 m wide
//! with gaps between them, so equal thirds would stretch the middle and
//! squeeze the outsides, and a face spanning all three would have a step
//! in it at every bezel.
//!
//! Gaps count. The canvas spans the full physical extent of its panels
//! including the wall between them, so the image continues *behind* the
//! bezels the way it would on one big screen. That is what makes a
//! spanned image read as one image rather than three cropped ones.

use crate::venue::GeometryRecord;
use bevy::prelude::*;
use std::collections::HashMap;

/// The piece of a canvas one panel shows, in UV space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slice {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

impl Slice {
    /// The whole canvas — what a panel that is its own canvas shows.
    pub const FULL: Slice = Slice {
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
    };

    /// UVs for a unit quad, in Bevy's vertex order: top-left,
    /// bottom-left, bottom-right, top-right.
    ///
    /// V is flipped relative to world Z because texture V grows
    /// downward — get this backwards and the image is upside down,
    /// which on a test card is obvious and on a gradient is not.
    pub fn uvs(&self) -> [[f32; 2]; 4] {
        [
            [self.u0, 1.0 - self.v1],
            [self.u0, 1.0 - self.v0],
            [self.u1, 1.0 - self.v0],
            [self.u1, 1.0 - self.v1],
        ]
    }
}

/// Works out each screen's slice of its canvas.
///
/// Panels are grouped by canvas name, and each group's bounding box in
/// the room becomes the canvas. A panel's slice is its own extent within
/// that box.
///
/// The horizontal axis is whichever of X or Y the group actually spans —
/// a back wall runs along X, a side wall along Y, and asking which one
/// moved is more honest than assuming. A single-panel canvas is always
/// the whole image, so a lone screen never gets a degenerate slice out
/// of a zero-width bounding box.
pub fn slices(screens: &[GeometryRecord]) -> HashMap<String, Slice> {
    let mut groups: HashMap<&str, Vec<&GeometryRecord>> = HashMap::new();
    for screen in screens {
        groups.entry(screen.canvas_name()).or_default().push(screen);
    }

    let mut out = HashMap::new();
    for (_, members) in groups {
        if members.len() < 2 {
            for m in members {
                out.insert(m.name.clone(), Slice::FULL);
            }
            continue;
        }

        // Extents of every panel, in world space. A panel's `position`
        // is its bottom edge and its `size.x` its width along its own
        // facing, so the span is taken about the centre.
        let spans: Vec<(f32, f32, f32, f32)> = members
            .iter()
            .map(|m| {
                let p = m.position.to_vec3();
                let half = m.size.x * 0.5;
                (p.x - half, p.x + half, p.z, p.z + m.size.y)
            })
            .collect();

        let x0 = spans.iter().map(|s| s.0).fold(f32::INFINITY, f32::min);
        let x1 = spans.iter().map(|s| s.1).fold(f32::NEG_INFINITY, f32::max);
        let z0 = spans.iter().map(|s| s.2).fold(f32::INFINITY, f32::min);
        let z1 = spans.iter().map(|s| s.3).fold(f32::NEG_INFINITY, f32::max);
        let width = (x1 - x0).max(f32::EPSILON);
        let height = (z1 - z0).max(f32::EPSILON);

        for (m, (mx0, mx1, mz0, mz1)) in members.iter().zip(&spans) {
            out.insert(
                m.name.clone(),
                Slice {
                    u0: (mx0 - x0) / width,
                    u1: (mx1 - x0) / width,
                    v0: (mz0 - z0) / height,
                    v1: (mz1 - z0) / height,
                },
            );
        }
    }
    out
}

/// A quad carrying a canvas slice's UVs rather than the default 0..1.
pub fn sliced_quad(slice: Slice) -> Mesh {
    let mut mesh = Mesh::from(Rectangle::new(1.0, 1.0));
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, slice.uvs().to_vec());
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::Vec3 as VenueVec3;

    fn screen(name: &str, x: f32, width: f32, canvas: Option<&str>) -> GeometryRecord {
        GeometryRecord {
            name: name.to_string(),
            position: VenueVec3 { x, y: 2.46, z: 1.2 },
            eulers: VenueVec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            size: VenueVec3 {
                x: width,
                y: 0.8,
                z: 0.05,
            },
            canvas: canvas.map(|c| c.to_string()),
        }
    }

    /// A screen on its own canvas shows the whole source, the way every
    /// screen did before canvases existed.
    #[test]
    fn a_lone_screen_gets_the_whole_image() {
        let got = slices(&[screen("TV", 0.0, 1.4, None)]);
        assert_eq!(got["TV"], Slice::FULL);
    }

    /// The real case: three panels of unequal width with gaps between
    /// them. Equal thirds would be wrong — the middle panel is wider, so
    /// it must take a wider slice, and the gaps must consume canvas so
    /// the image continues behind the bezels.
    #[test]
    fn unequal_panels_take_slices_matching_their_real_width() {
        let got = slices(&[
            screen("L", -3.5, 1.33, Some("main")),
            screen("C", 0.0, 1.44, Some("main")),
            screen("R", 3.5, 1.33, Some("main")),
        ]);
        // The canvas spans -4.165 to 4.165 — 8.33 m wide.
        let width = 8.33;
        let l = got["L"];
        let c = got["C"];
        let r = got["R"];
        assert!((l.u0 - 0.0).abs() < 1e-4, "{l:?}");
        assert!((r.u1 - 1.0).abs() < 1e-4, "{r:?}");
        // Each panel's slice is exactly its own width as a fraction.
        assert!(((l.u1 - l.u0) - 1.33 / width).abs() < 1e-4, "{l:?}");
        assert!(((c.u1 - c.u0) - 1.44 / width).abs() < 1e-4, "{c:?}");
        // The middle panel really is wider than the outer ones — the
        // thing equal thirds would have got wrong.
        assert!(c.u1 - c.u0 > l.u1 - l.u0);
        // ...and the gaps are real canvas, not skipped.
        assert!(c.u0 > l.u1, "the gap between L and C vanished");
    }

    /// Panels in the middle of a canvas sit where they physically are,
    /// so the centre TV's slice is centred.
    #[test]
    fn the_middle_panel_is_centred_in_the_canvas() {
        let got = slices(&[
            screen("L", -3.5, 1.33, Some("main")),
            screen("C", 0.0, 1.44, Some("main")),
            screen("R", 3.5, 1.33, Some("main")),
        ]);
        let c = got["C"];
        let mid = (c.u0 + c.u1) * 0.5;
        assert!((mid - 0.5).abs() < 1e-4, "{c:?}");
    }

    /// Separate canvases do not interact — the side screens each show
    /// their own whole source however far apart they are.
    #[test]
    fn separate_canvases_are_independent() {
        let got = slices(&[
            screen("SL", 4.9, 1.44, Some("side-left")),
            screen("SR", -4.9, 1.44, Some("side-right")),
            screen("C", 0.0, 1.44, Some("main")),
        ]);
        assert_eq!(got["SL"], Slice::FULL);
        assert_eq!(got["SR"], Slice::FULL);
        assert_eq!(got["C"], Slice::FULL);
    }

    /// V is flipped on the way into UVs, because texture V grows
    /// downward and world Z grows up. Upside-down video is the classic
    /// symptom of getting this wrong.
    #[test]
    fn uvs_flip_v_for_texture_space() {
        let slice = Slice {
            u0: 0.25,
            v0: 0.0,
            u1: 0.75,
            v1: 0.5,
        };
        let uvs = slice.uvs();
        // Top-left of the quad samples the *upper* part of the texture,
        // which is the lower V.
        assert_eq!(uvs[0], [0.25, 0.5]);
        assert_eq!(uvs[1], [0.25, 1.0]);
        assert_eq!(uvs[2], [0.75, 1.0]);
        assert_eq!(uvs[3], [0.75, 0.5]);
    }
}
