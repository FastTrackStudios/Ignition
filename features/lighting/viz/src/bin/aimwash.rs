//! Re-aims static wash fixtures at face height.
//!
//! A par has no motors, so its *hang* is its aim: the tilt baked into
//! `fixtures.json` is the whole story. Norco's survey had the front
//! washes landing on the talent's chest — beam centre at 1.26 m when it
//! reached the vocal line, where a face is at about 1.70 m. On the real
//! rig they are angled higher, which is the point of front light: you
//! are lighting faces, and a wash centred on a sternum lights a shirt
//! and leaves the eyes in shadow.
//!
//! This recomputes the tilt so the beam centre passes through
//! `--face` metres at the performer line, and writes both `eulers` and
//! `quat` — `FixtureRecord::orientation()` reads the quaternion, so an
//! angle edit on its own is silently ignored.
//!
//! Deliberately narrow about what it touches. Only fixtures already
//! tilted past `--min-tilt` are front washes; the near-vertical ones
//! (Norco has several at 15°) are downlights doing a different job, and
//! swinging those onto the vocal line would wreck a working rig to fix a
//! problem they do not have.
//!
//! ```text
//! cargo run -p ignition-viz --bin aimwash -- data/venues/norco/fixtures.json
//! cargo run -p ignition-viz --bin aimwash -- <path> --face 1.6 --write
//! ```
//!
//! Prints a table and changes nothing without `--write`.

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Where the talent stands, in venue Y. Norco's "Vocal Centre" focus
/// point, which is the line the front wash exists to light.
const PERFORMER_Y: f64 = -3.44;
/// Beam centre height to aim for. The models are 1.83 m, so this is
/// face rather than the top of the head — a wash centred on the crown
/// throws half its output over the talent and into the back wall.
const FACE: f64 = 1.70;
/// Below this tilt a fixture is a downlight, not a front wash.
const MIN_TILT: f64 = 40.0;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .context("usage: aimwash <fixtures.json> [--face M] [--min-tilt D] [--write]")?;
    let (mut face, mut min_tilt, mut write) = (FACE, MIN_TILT, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--write" => write = true,
            "--face" => face = args.next().context("--face needs a value")?.parse()?,
            "--min-tilt" => min_tilt = args.next().context("--min-tilt needs a value")?.parse()?,
            other => bail!("unknown argument {other}"),
        }
    }

    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
    let mut fixtures: Value = serde_json::from_str(&text)?;
    let list = fixtures.as_array_mut().context("expected an array")?;

    let mut changed = 0usize;
    println!(
        "{:>5} {:>7} {:>7} {:>9} {:>9}",
        "chan", "tilt", "new", "was@face", "role"
    );
    for fixture in list.iter_mut() {
        let Some(record) = Aim::read(fixture) else {
            continue;
        };
        if !record.is_wash {
            continue;
        }
        // Behind the performer line this is a back light, and "higher"
        // would aim it into the audience.
        if record.y >= PERFORMER_Y || record.z <= face {
            continue;
        }
        if record.tilt < min_tilt {
            println!(
                "{:>5} {:>7.1} {:>7} {:>9} {:>9}",
                record.chan, record.tilt, "-", "-", "downlight"
            );
            continue;
        }

        // Only fixtures that undershoot. A wash already clearing face
        // height at the vocal line is aimed somewhere else — upstage
        // band, back wall, the audience — and "aim at the singer's face"
        // is not an instruction it should be given. Without this guard
        // the tool cheerfully swings Norco's 60-degree upstage bar down
        // to 11 degrees, which is not a correction, it is vandalism.
        let height = record.height_at(PERFORMER_Y);
        if height >= face {
            continue;
        }
        let wanted = (PERFORMER_Y - record.y).atan2(record.z - face).to_degrees();
        if (wanted - record.tilt).abs() < 0.05 {
            continue;
        }
        println!(
            "{:>5} {:>7.1} {:>7.1} {:>9.2} {:>9}",
            record.chan, record.tilt, wanted, height, "wash"
        );
        if write {
            record.apply(fixture, wanted);
            changed = changed.saturating_add(1);
        }
    }

    if write {
        // Trailing newline: the file is committed, and a diff whose last
        // line is "\ No newline at end of file" is noise every time.
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&fixtures)?),
        )?;
        println!("\nre-aimed {changed} fixtures at {face} m -> {path}");
    } else {
        println!("\ndry run — pass --write to apply");
    }
    Ok(())
}

/// The fields this tool needs out of one fixture record.
struct Aim {
    chan: i64,
    y: f64,
    z: f64,
    tilt: f64,
    rot_z: f64,
    is_wash: bool,
}

impl Aim {
    fn read(fixture: &Value) -> Option<Self> {
        let position = fixture.get("position")?;
        let eulers = fixture.get("eulers")?;
        Some(Self {
            chan: fixture.get("chan")?.as_i64().unwrap_or(0),
            y: position.get("y")?.as_f64()?,
            z: position.get("z")?.as_f64()?,
            tilt: eulers.get("x")?.as_f64()?,
            rot_z: eulers.get("z").and_then(Value::as_f64).unwrap_or(0.0),
            is_wash: fixture
                .get("tags")
                .and_then(Value::as_array)
                .is_some_and(|tags| {
                    tags.iter()
                        .filter_map(Value::as_str)
                        .any(|t| t.contains("Wash"))
                }),
        })
    }

    /// Beam-centre height when the beam reaches `y`.
    fn height_at(&self, y: f64) -> f64 {
        let t = self.tilt.to_radians().tan();
        if t <= 1e-6 {
            return f64::INFINITY;
        }
        self.z - (y - self.y) / t
    }

    /// Writes the new tilt back as both angles and quaternion.
    ///
    /// Identity is "hung from the truss" — pointing straight down — so
    /// tilt is degrees from nadir, and the composition order matches
    /// `venue::euler_to_quat`'s `EulerRot::ZYX`.
    fn apply(&self, fixture: &mut Value, tilt_deg: f64) {
        let (z, x) = (self.rot_z.to_radians(), tilt_deg.to_radians());
        let (sz, cz) = (z * 0.5).sin_cos();
        let (sx, cx) = (x * 0.5).sin_cos();
        // ZYX with a zero Y term: q = Rz * Rx.
        let [w, qx, qy, qz] = [cz * cx, cz * sx, sz * sx, sz * cx]; // w, x, y, z

        // `fixture[...][...] = ...` is `serde_json`'s panicking `IndexMut`;
        // this is a fixture record read off disk, so a malformed "eulers"
        // or "quat" (present but not an object) is data to route around,
        // not a crash. `entry(...).or_insert_with(...)` preserves the one
        // behaviour the old indexing gave us for free — an absent field is
        // created as an object rather than left missing.
        let Some(root) = fixture.as_object_mut() else {
            return;
        };
        let eulers = root
            .entry("eulers")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(eulers) = eulers.as_object_mut() {
            eulers.insert("x".to_string(), round6(tilt_deg));
        }
        let quat = root
            .entry("quat")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(quat) = quat.as_object_mut() {
            quat.insert("w".to_string(), round6(w));
            quat.insert("x".to_string(), round6(qx));
            quat.insert("y".to_string(), round6(qy));
            quat.insert("z".to_string(), round6(qz));
        }
    }
}

fn round6(v: f64) -> Value {
    serde_json::json!((v * 1e6).round() / 1e6)
}
