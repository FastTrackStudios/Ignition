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
use ignition_core::canvas::CanvasRecipe;
use ignition_core::step::SpeedMasters;
use std::collections::HashMap;

/// What a canvas content string starts with to mean "generate this"
/// rather than "load this file".
pub const PROC_PREFIX: &str = "proc:";

/// The widest a procedural frame is rendered by the CPU reference
/// ([`ProceduralSource`]). The screens themselves no longer go through
/// it — `canvas_material.rs` evaluates the recipe on the GPU at native
/// resolution — so this is the size a test or an offline consumer gets.
pub const PROC_MAX_WIDTH: u32 = 320;

/// Reads a `proc:` content string.
///
/// Two spellings: `proc:<name>` for one of the built-in recipes
/// (`rainbow`, `wipe`, `noise`, `bands`, `sparkle`), or `proc:{...}` —
/// a JSON `CanvasRecipe` literal, which is what a show file carries.
// r[impl canvas.procedural] - a gradient or a wipe needs no file, only a string
pub fn parse_procedural(content: &str) -> Result<CanvasRecipe, String> {
    let body = content
        .strip_prefix(PROC_PREFIX)
        .ok_or_else(|| format!("not a procedural source: {content}"))?
        .trim();
    if body.starts_with('{') {
        serde_json::from_str(body).map_err(|e| format!("canvas recipe JSON: {e}"))
    } else {
        ignition_core::canvas::named(body)
            .ok_or_else(|| format!("no built-in canvas recipe named {body:?}"))
    }
}

/// A generated canvas source on the CPU: the **reference** picture,
/// presented against the same clock as a clip and handing over frames
/// of the same shape.
///
/// Nothing here knows about Bevy: `frame_at` returns bytes. The
/// visualizer's screens do not use it any more — `canvas_material.rs`
/// paints the same recipe on the GPU, and its test holds that picture
/// to this one — but a bitmap channel, the cooker and anything without
/// a GPU do.
// r[impl canvas.clip-is-a-source] - same clock, same frame shape
// r[impl canvas.procedural] - the CPU reference the GPU is held to
pub struct ProceduralSource {
    recipe: CanvasRecipe,
    width: u32,
    height: u32,
    masters: SpeedMasters,
    frame: Vec<u8>,
    last_cycles: Option<f32>,
}

impl ProceduralSource {
    /// A source sized to `canvas_aspect` (width over height) at
    /// `PROC_MAX_WIDTH`, so cover-fitting it to its canvas is a no-op
    /// and every pixel rendered lands on a panel.
    pub fn new(recipe: CanvasRecipe, canvas_aspect: f32) -> Self {
        let aspect = if canvas_aspect.is_finite() && canvas_aspect > 0.0 {
            canvas_aspect
        } else {
            16.0 / 9.0
        };
        let width = PROC_MAX_WIDTH;
        let height = ((width as f32 / aspect).round() as u32).clamp(1, PROC_MAX_WIDTH * 4);
        Self {
            recipe,
            width,
            height,
            masters: SpeedMasters::new(),
            frame: Vec::new(),
            last_cycles: None,
        }
    }

    /// The speed masters the recipe's `Speed::Master` resolves against.
    pub fn set_masters(&mut self, masters: SpeedMasters) {
        self.masters = masters;
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The frame for song position `secs`, or `None` when the clock
    /// has not moved the picture since the last one — a stopped
    /// transport must not re-upload the same texture every render.
    ///
    /// Renders on the calling thread. The visualizer uses `advance` and
    /// `render_cycles` instead, so the raster runs on a worker and the
    /// frame is never held up by it.
    pub fn frame_at(&mut self, secs: f64) -> Option<&[u8]> {
        let cycles = self.advance(secs)?;
        self.frame = self.render_cycles(cycles);
        Some(self.frame.as_slice())
    }

    /// Where the recipe is at `secs`, or `None` when it is where it was
    /// last time — the picture has not moved and nothing needs drawing.
    pub fn advance(&mut self, secs: f64) -> Option<f32> {
        let cycles = self.recipe.cycles_at(secs as f32, &self.masters);
        if self.last_cycles == Some(cycles) {
            return None;
        }
        self.last_cycles = Some(cycles);
        Some(cycles)
    }

    /// The picture at `cycles`, RGBA at `size()`.
    pub fn render_cycles(&self, cycles: f32) -> Vec<u8> {
        self.recipe.render(self.width, self.height, cycles)
    }

    /// The recipe, for a worker to render with.
    pub fn recipe(&self) -> &CanvasRecipe {
        &self.recipe
    }
}

/// The piece of a canvas one panel shows, in UV space.
#[derive(Debug, Clone, Copy, PartialEq)]
// r[impl files.venue.screens] - slices are derived from real panel geometry, not stored
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

    /// UVs for a unit quad, in Bevy's `Rectangle` vertex order:
    /// **top-right, top-left, bottom-left, bottom-right** (see
    /// `bevy_mesh::primitives::dim2::RectangleMeshBuilder`).
    ///
    /// This used to assume top-left first, which is one vertex out —
    /// and one vertex out on a quad is the picture rolled ninety
    /// degrees on every screen in the room, a test card reading
    /// top-to-bottom. V is flipped relative to world Z because texture V
    /// grows downward.
    pub fn uvs(&self) -> [[f32; 2]; 4] {
        [
            [self.u1, 1.0 - self.v1],
            [self.u0, 1.0 - self.v1],
            [self.u0, 1.0 - self.v0],
            [self.u1, 1.0 - self.v0],
        ]
    }
}

impl Slice {
    /// This slice, of a source **cover-fitted** to its canvas.
    ///
    /// A canvas is three TVs wide and one high; a clip is 16:9. Mapping
    /// the whole clip onto the whole canvas squashes it to a strip that
    /// reads as rotated. Cover-fitting scales the source uniformly until
    /// it fills the canvas and crops what overflows, centred — so a
    /// wide canvas shows the middle band of the clip and a tall one the
    /// middle column, and nothing is ever stretched. `canvas_aspect` and
    /// `source_aspect` are width over height.
    // r[impl files.venue.canvases] - a canvas shows a centred crop of its source, never a stretch
    pub fn cover(self, canvas_aspect: f32, source_aspect: f32) -> Slice {
        self.cover_at(canvas_aspect, source_aspect, 0.5)
    }

    /// `cover`, with the crop centred on `focus` (0 = the source's top
    /// or left edge, 1 = its bottom or right) rather than its middle —
    /// so a wide canvas can show the band with the face in it. The band
    /// is kept inside the source, so a focus near an edge pins to it.
    pub fn cover_at(self, canvas_aspect: f32, source_aspect: f32, focus: f32) -> Slice {
        if !(canvas_aspect > 0.0) || !(source_aspect > 0.0) {
            return self;
        }
        let focus = focus.clamp(0.0, 1.0);
        let band = |t: f32, f: f32| {
            let centre = focus.clamp(f * 0.5, 1.0 - f * 0.5);
            centre + (t - 0.5) * f
        };
        if source_aspect < canvas_aspect {
            // The source is taller than the canvas: full width, a
            // horizontal band of the source's height.
            let f = source_aspect / canvas_aspect;
            Slice {
                u0: self.u0,
                u1: self.u1,
                v0: band(self.v0, f),
                v1: band(self.v1, f),
            }
        } else {
            let f = canvas_aspect / source_aspect;
            Slice {
                u0: band(self.u0, f),
                u1: band(self.u1, f),
                v0: self.v0,
                v1: self.v1,
            }
        }
    }
}

/// Each canvas's aspect, width over height, from the panels' real
/// geometry — a lone panel's own size, or the bounding box of a group.
pub fn canvas_aspects(screens: &[GeometryRecord]) -> HashMap<String, f32> {
    let mut groups: HashMap<&str, Vec<&GeometryRecord>> = HashMap::new();
    for screen in screens {
        groups.entry(screen.canvas_name()).or_default().push(screen);
    }
    groups
        .into_iter()
        .map(|(canvas, members)| {
            let x0 = members
                .iter()
                .map(|m| m.position.x as f32 - m.size.x * 0.5)
                .fold(f32::INFINITY, f32::min);
            let x1 = members
                .iter()
                .map(|m| m.position.x as f32 + m.size.x * 0.5)
                .fold(f32::NEG_INFINITY, f32::max);
            let z0 = members
                .iter()
                .map(|m| m.position.z as f32)
                .fold(f32::INFINITY, f32::min);
            let z1 = members
                .iter()
                .map(|m| m.position.z as f32 + m.size.y)
                .fold(f32::NEG_INFINITY, f32::max);
            let aspect = (x1 - x0).max(f32::EPSILON) / (z1 - z0).max(f32::EPSILON);
            (canvas.to_string(), aspect)
        })
        .collect()
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
// r[impl files.venue.screens]
// r[impl files.venue.canvases] - the venue decides how many panels a canvas has and how wide each is
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
    use ignition_core::canvas::Procedural;
    use ignition_core::step::{Speed, Timing};

    /// A frame comes out of a procedural source with no Bevy, no file
    /// and no clock of its own — sized to the canvas, RGBA, and only
    /// re-rendered when the picture actually moved.
    #[test]
    /// r[verify canvas.procedural]
    /// r[verify canvas.clip-is-a-source]
    fn a_procedural_source_renders_a_frame_offline() {
        let recipe = CanvasRecipe {
            source: Procedural::Wipe {
                color: [1.0; 3],
                width: 0.2,
                direction: Default::default(),
            },
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Timing::default()
            },
        };
        let mut src = ProceduralSource::new(recipe, 8.33 / 0.8);
        let (w, h) = src.size();
        assert_eq!(w, PROC_MAX_WIDTH);
        assert!(h > 0 && h < w);
        let first = src.frame_at(0.0).expect("a first frame").to_vec();
        assert_eq!(first.len(), (w * h * 4) as usize);
        assert!(first.iter().any(|&p| p == 255));
        // Same time, same picture — nothing to upload.
        assert!(src.frame_at(0.0).is_none());
        // Time moved: a different picture.
        let later = src.frame_at(0.4).expect("a moved frame");
        assert_ne!(later, first.as_slice());
    }

    #[test]
    /// r[verify canvas.procedural]
    fn proc_strings_parse_by_name_or_as_json() {
        assert!(parse_procedural("proc:rainbow").is_ok());
        let json = r#"proc:{"source":{"Gradient":{"colors":[[1,0,0],[0,0,1]],"angle_deg":0}},"timing":{"speed":{"Master":"Song"},"measure":4}}"#;
        let r = parse_procedural(json).expect("a JSON recipe");
        assert_eq!(r.timing.speed, Speed::Master("Song".into()));
        assert!(parse_procedural("proc:nope").is_err());
        assert!(parse_procedural("clips/city.mp4").is_err());
    }

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
    /// r[verify files.venue.canvases]
    fn a_lone_screen_gets_the_whole_image() {
        let got = slices(&[screen("TV", 0.0, 1.4, None)]);
        assert_eq!(got["TV"], Slice::FULL);
    }

    /// The real case: three panels of unequal width with gaps between
    /// them. Equal thirds would be wrong — the middle panel is wider, so
    /// it must take a wider slice, and the gaps must consume canvas so
    /// the image continues behind the bezels.
    #[test]
    /// r[verify files.venue.screens]
    /// r[verify files.venue.canvases]
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
    /// r[verify files.venue.screens]
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
    /// r[verify files.venue.canvases]
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

    /// A 16:9 clip on a canvas ten times wider than it is tall shows
    /// its middle band at full width, never a squash.
    #[test]
    fn a_wide_canvas_shows_the_middle_band_of_a_clip() {
        let s = Slice::FULL.cover(10.0, 16.0 / 9.0);
        assert!(
            (s.u0 - 0.0).abs() < 1e-6 && (s.u1 - 1.0).abs() < 1e-6,
            "{s:?}"
        );
        let f = (16.0 / 9.0) / 10.0;
        assert!((s.v0 - (0.5 - f / 2.0)).abs() < 1e-5, "{s:?}");
        assert!((s.v1 - (0.5 + f / 2.0)).abs() < 1e-5, "{s:?}");
    }

    /// A portrait clip on a landscape TV fills the width and shows its
    /// middle band — never squashed, never rolled.
    #[test]
    fn a_tall_clip_on_a_wide_panel_is_cropped_not_rotated() {
        let s = Slice::FULL.cover(16.0 / 9.0, 9.0 / 16.0);
        assert!((s.u0).abs() < 1e-6 && (s.u1 - 1.0).abs() < 1e-6, "{s:?}");
        assert!(s.v0 > 0.3 && s.v1 < 0.7, "{s:?}");
    }

    /// A focus below the middle slides the band down, and it never
    /// leaves the source.
    #[test]
    fn a_focus_moves_the_band_and_stays_inside_the_frame() {
        let mid = Slice::FULL.cover_at(10.0, 16.0 / 9.0, 0.5);
        let low = Slice::FULL.cover_at(10.0, 16.0 / 9.0, 0.65);
        assert!(low.v0 > mid.v0 && (low.v1 - low.v0 - (mid.v1 - mid.v0)).abs() < 1e-5);
        let edge = Slice::FULL.cover_at(10.0, 16.0 / 9.0, 1.0);
        assert!((edge.v1 - 1.0).abs() < 1e-5, "{edge:?}");
    }

    /// A wider-than-canvas source shows its middle columns.
    #[test]
    fn a_wide_clip_on_a_squarer_canvas_is_cropped_sideways() {
        let s = Slice::FULL.cover(1.0, 2.0);
        assert!((s.v0).abs() < 1e-6 && (s.v1 - 1.0).abs() < 1e-6, "{s:?}");
        assert!(
            (s.u0 - 0.25).abs() < 1e-6 && (s.u1 - 0.75).abs() < 1e-6,
            "{s:?}"
        );
    }

    /// A panel's own slice is cropped consistently with its neighbours,
    /// so the three pieces still join into one picture.
    #[test]
    fn covered_slices_still_tile() {
        let a = Slice {
            u0: 0.0,
            u1: 0.5,
            v0: 0.0,
            v1: 1.0,
        }
        .cover(8.0, 2.0);
        let b = Slice {
            u0: 0.5,
            u1: 1.0,
            v0: 0.0,
            v1: 1.0,
        }
        .cover(8.0, 2.0);
        assert!((a.u1 - b.u0).abs() < 1e-6);
        assert!((a.v0 - b.v0).abs() < 1e-6 && (a.v1 - b.v1).abs() < 1e-6);
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
        // Bevy's order: top-right, top-left, bottom-left, bottom-right.
        assert_eq!(uvs[0], [0.75, 0.5]);
        assert_eq!(uvs[1], [0.25, 0.5]);
        assert_eq!(uvs[2], [0.25, 1.0]);
        assert_eq!(uvs[3], [0.75, 1.0]);
    }
}
