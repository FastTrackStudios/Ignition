//! Loads the extracted venue JSON (`data/venues/<name>/*.json`) — see
//! `docs/domain/norco-venue-reference.md` for what each file contains.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn to_glam(self) -> glam::Vec3 {
        glam::Vec3::new(self.x, self.y, self.z)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quat {
    pub fn to_glam(self) -> glam::Quat {
        glam::Quat::from_xyzw(self.x, self.y, self.z, self.w).normalize()
    }
}

fn euler_to_glam(e: Vec3) -> glam::Quat {
    glam::Quat::from_euler(
        glam::EulerRot::ZYX,
        e.z.to_radians(),
        e.y.to_radians(),
        e.x.to_radians(),
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct FixtureRecord {
    pub chan: Option<u32>,
    pub name: String,
    pub tags: Vec<String>,
    #[serde(default = "default_patched")]
    pub patched: bool,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub position: Vec3,
    pub eulers: Vec3,
    pub quat: Quat,
    pub size: Vec3,
}

fn default_patched() -> bool {
    true
}

impl FixtureRecord {
    /// The mounting orientation — the *hang*, never the aim. See
    /// `docs/domain/norco-venue-reference.md`.
    pub fn orientation(&self) -> glam::Quat {
        self.quat.to_glam()
    }

    pub fn kind(&self) -> FixtureKind {
        if self.tags.iter().any(|t| t.contains("Yoke") || t.contains("Mover")) {
            FixtureKind::Mover
        } else if self.tags.iter().any(|t| t.contains("Wash")) {
            FixtureKind::Wash
        } else {
            FixtureKind::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    Wash,
    Mover,
    Other,
}

impl FixtureKind {
    pub fn color(self) -> [f32; 3] {
        match self {
            FixtureKind::Wash => [0.25, 0.75, 0.95],
            FixtureKind::Mover => [0.95, 0.55, 0.15],
            FixtureKind::Other => [0.65, 0.65, 0.70],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeometryRecord {
    pub name: String,
    pub position: Vec3,
    pub eulers: Vec3,
    pub size: Vec3,
}

impl GeometryRecord {
    pub fn orientation(&self) -> glam::Quat {
        euler_to_glam(self.eulers)
    }
}

pub struct Venue {
    pub fixtures: Vec<FixtureRecord>,
    pub room: Vec<GeometryRecord>,
    pub screens: Vec<GeometryRecord>,
    pub props: Vec<GeometryRecord>,
}

impl Venue {
    pub fn load(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        let read = |name: &str| -> anyhow::Result<String> {
            std::fs::read_to_string(dir.join(name))
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", dir.join(name).display()))
        };
        Ok(Self {
            fixtures: serde_json::from_str(&read("fixtures.json")?)?,
            room: serde_json::from_str(&read("room.json")?)?,
            screens: serde_json::from_str(&read("screens.json")?)?,
            props: serde_json::from_str(&read("props.json")?)?,
        })
    }

    /// Axis-aligned bounds over every object's centre — used to auto-frame
    /// the default camera regardless of which venue is loaded.
    pub fn bounds(&self) -> (glam::Vec3, glam::Vec3) {
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        let mut visit = |p: glam::Vec3| {
            min = min.min(p);
            max = max.max(p);
        };
        for f in &self.fixtures {
            visit(f.position.to_glam());
        }
        for g in self.room.iter().chain(&self.screens).chain(&self.props) {
            visit(g.position.to_glam());
        }
        (min, max)
    }
}
