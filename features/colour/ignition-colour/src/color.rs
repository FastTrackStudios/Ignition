//! Colour as *intent* — a device-independent value a preset stores — and
//! the solve that turns intent into emitter levels for one real fixture.
//!
//! `preset.rs::ColorPreset` keeps its linear RGB triple (every show file
//! ever written carries one); this module is what lets it also say
//! "warm white at 3200 K" or "Lee 201", and what lets a four-emitter wash
//! reach that colour with its white instead of a dim RGB approximation.
//! The solve happens at output time (`ignition_viz::show`), against the
//! fixture's own emitters, which is the whole point of storing intent.

use serde::{Deserialize, Serialize};

/// A linear-light RGB triple (sRGB primaries, D65 white), each channel
/// nominally `0..=1`. The value form of `r[color.model]`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
// r[impl color.model] - a linear RGB triple, interconvertible with HSB
pub struct Rgb {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

/// Hue in degrees `0..360`, saturation and brightness in `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hsb {
    pub hue: f32,
    pub saturation: f32,
    pub brightness: f32,
}

/// CIE 1931 xyY: chromaticity `(x, y)` plus luminance `Y` (relative,
/// `1.0` = the fixture's full white).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Xyy {
    pub x: f32,
    pub y: f32,
    #[serde(rename = "Y")]
    pub luminance: f32,
}

/// CIE 1931 XYZ tristimulus.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Xyz {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// What a colour *means*, independent of any fixture. Every variant
/// converts to `Xyy` (`Intent::xyy`) and to the RGB triple old files
/// need (`Intent::rgb`); the gel variant keeps its reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
// r[impl color.intent] - xy chromaticity, CCT or gel, beyond RGB
// r[impl color.space-independent] - none of these is an emitter level
pub enum Intent {
    /// Linear sRGB.
    Rgb(Rgb),
    /// Chromaticity plus luminance.
    Xy {
        x: f32,
        y: f32,
        #[serde(rename = "Y", default = "one")]
        luminance: f32,
    },
    /// Correlated colour temperature in kelvin, with a green/magenta
    /// `tint` in roughly CIE 1960 Duv units × 100 (0 = on the locus,
    /// positive = greener).
    Cct {
        kelvin: f32,
        #[serde(default)]
        tint: f32,
    },
    /// A manufacturer's swatch — `Gel { manufacturer: "Lee", number: "201" }`.
    // r[impl color.gel] - the reference is what is stored
    Gel {
        manufacturer: String,
        number: String,
    },
}

fn one() -> f32 {
    1.0
}

/// One additive light source in a fixture: where it sits on the
/// chromaticity diagram and how much it puts out at full. GDTF's
/// `Emitter` node carries exactly this (`ColorCIE` = x, y, Y).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    pub name: String,
    pub x: f32,
    pub y: f32,
    /// Relative output at full; only the ratio between a fixture's
    /// emitters matters to the solve.
    pub max_lumens: f32,
}

impl Emitter {
    /// Full-output tristimulus of this emitter.
    pub fn xyz(&self) -> Xyz {
        Xyy {
            x: self.x,
            y: self.y,
            luminance: self.max_lumens,
        }
        .to_xyz()
    }
}

/// A named emitter — the constructor the tables in `ignition_viz` use.
pub fn emitter(name: &str, x: f32, y: f32, max_lumens: f32) -> Emitter {
    Emitter {
        name: name.to_string(),
        x,
        y,
        max_lumens,
    }
}

// --- RGB <-> HSB ---------------------------------------------------------

impl Rgb {
    pub const fn new(red: f32, green: f32, blue: f32) -> Rgb {
        Rgb { red, green, blue }
    }

    pub const WHITE: Rgb = Rgb::new(1.0, 1.0, 1.0);

    // r[impl color.model] - RGB <-> HSB without loss beyond float precision
    pub fn to_hsb(self) -> Hsb {
        let max = self.red.max(self.green).max(self.blue);
        let min = self.red.min(self.green).min(self.blue);
        let delta = max - min;
        let hue = if delta <= f32::EPSILON {
            0.0
        } else if max == self.red {
            60.0 * (((self.green - self.blue) / delta).rem_euclid(6.0))
        } else if max == self.green {
            60.0 * ((self.blue - self.red) / delta + 2.0)
        } else {
            60.0 * ((self.red - self.green) / delta + 4.0)
        };
        Hsb {
            hue,
            saturation: if max <= 0.0 { 0.0 } else { delta / max },
            brightness: max,
        }
    }

    pub fn from_hsb(hsb: Hsb) -> Rgb {
        let c = hsb.brightness * hsb.saturation;
        let h = (hsb.hue.rem_euclid(360.0)) / 60.0;
        let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = hsb.brightness - c;
        Rgb::new(r + m, g + m, b + m)
    }

    /// Linear sRGB -> XYZ (IEC 61966-2-1, D65).
    pub fn to_xyz(self) -> Xyz {
        Xyz {
            x: 0.4124564 * self.red + 0.3575761 * self.green + 0.1804375 * self.blue,
            y: 0.2126729 * self.red + 0.7151522 * self.green + 0.0721750 * self.blue,
            z: 0.0193339 * self.red + 0.119_192 * self.green + 0.9503041 * self.blue,
        }
    }

    pub fn to_xyy(self) -> Xyy {
        self.to_xyz().to_xyy()
    }

    /// Every channel clamped to `0..=1`.
    pub fn clamped(self) -> Rgb {
        Rgb::new(
            self.red.clamp(0.0, 1.0),
            self.green.clamp(0.0, 1.0),
            self.blue.clamp(0.0, 1.0),
        )
    }
}

impl Xyz {
    /// XYZ -> linear sRGB (inverse of `Rgb::to_xyz`). Not clamped: an
    /// out-of-gamut colour comes back with a negative channel, and the
    /// caller decides what to do about it.
    pub fn to_rgb(self) -> Rgb {
        Rgb {
            red: 3.2404542 * self.x - 1.5371385 * self.y - 0.4985314 * self.z,
            green: -0.969_266 * self.x + 1.8760108 * self.y + 0.0415560 * self.z,
            blue: 0.0556434 * self.x - 0.2040259 * self.y + 1.0572252 * self.z,
        }
    }

    pub fn to_xyy(self) -> Xyy {
        let sum = self.x + self.y + self.z;
        if sum <= 1e-9 {
            // Black has no chromaticity; give it D65's so it stays a
            // valid point.
            return Xyy {
                x: 0.3127,
                y: 0.3290,
                luminance: 0.0,
            };
        }
        Xyy {
            x: self.x / sum,
            y: self.y / sum,
            luminance: self.y,
        }
    }
}

impl Xyy {
    pub fn to_xyz(self) -> Xyz {
        if self.y <= 1e-9 {
            return Xyz::default();
        }
        Xyz {
            x: self.x * self.luminance / self.y,
            y: self.luminance,
            z: (1.0 - self.x - self.y) * self.luminance / self.y,
        }
    }

    /// The RGB triple that renders this colour, scaled into gamut: an
    /// out-of-gamut chromaticity is desaturated toward its own luminance
    /// rather than clipped per channel, so the hue survives.
    pub fn to_rgb(self) -> Rgb {
        let rgb = self.to_xyz().to_rgb();
        let min = rgb.red.min(rgb.green).min(rgb.blue);
        let rgb = if min < 0.0 {
            // Mix in enough white (equal RGB, at this luminance) to lift the
            // negative channel to zero.
            let white = Rgb::new(self.luminance, self.luminance, self.luminance);
            let t = (-min / (self.luminance - min).max(1e-6)).clamp(0.0, 1.0);
            Rgb::new(
                rgb.red + (white.red - rgb.red) * t,
                rgb.green + (white.green - rgb.green) * t,
                rgb.blue + (white.blue - rgb.blue) * t,
            )
        } else {
            rgb
        };
        let max = rgb.red.max(rgb.green).max(rgb.blue);
        if max > 1.0 {
            Rgb::new(rgb.red / max, rgb.green / max, rgb.blue / max)
        } else {
            rgb.clamped()
        }
    }
}

// --- CCT -----------------------------------------------------------------

/// Planckian-locus chromaticity for a colour temperature, Kim et al.
/// (2002) cubic spline, valid 1667 K – 25000 K; clamped outside.
/// `tint` shifts perpendicular to the locus in CIE 1960 (u, v), in
/// hundredths of Duv, positive toward green.
// r[impl color.cct] - a temperature is a point on the Planckian locus
pub fn cct_to_xy(kelvin: f32, tint: f32) -> (f32, f32) {
    let t = (kelvin as f64).clamp(1667.0, 25000.0);
    let locus = |t: f64| -> (f64, f64) {
        let t2 = t * t;
        let t3 = t2 * t;
        let x = if t <= 4000.0 {
            -0.2661239e9 / t3 - 0.2343589e6 / t2 + 0.8776956e3 / t + 0.179910
        } else {
            -3.0258469e9 / t3 + 2.1070379e6 / t2 + 0.2226347e3 / t + 0.240390
        };
        let x2 = x * x;
        let x3 = x2 * x;
        let y = if t <= 2222.0 {
            -1.1063814 * x3 - 1.34811020 * x2 + 2.18555832 * x - 0.20219683
        } else if t <= 4000.0 {
            -0.9549476 * x3 - 1.37418593 * x2 + 2.09137015 * x - 0.16748867
        } else {
            3.0817580 * x3 - 5.87338670 * x2 + 3.75112997 * x - 0.37001483
        };
        (x, y)
    };
    let (x, y) = locus(t);
    if tint.abs() < 1e-6 {
        return (x as f32, y as f32);
    }
    // Perpendicular to the locus in uv, found from a numerical tangent.
    let to_uv = |(x, y): (f64, f64)| {
        let d = -2.0 * x + 12.0 * y + 3.0;
        (4.0 * x / d, 6.0 * y / d)
    };
    let (u0, v0) = to_uv((x, y));
    let (u1, v1) = to_uv(locus(t * 1.01));
    let (du, dv) = (u1 - u0, v1 - v0);
    let len = (du * du + dv * dv).sqrt().max(1e-9);
    // Rotate the tangent 90° so +tint moves above the locus (greener).
    let (nu, nv) = (dv / len, -du / len);
    let duv = tint as f64 * 0.01;
    let (u, v) = (u0 + nu * duv, v0 + nv * duv);
    let d = 2.0 * u - 8.0 * v + 4.0;
    ((3.0 * u / d) as f32, (2.0 * v / d) as f32)
}

// --- Gels ----------------------------------------------------------------

/// A swatch entry: manufacturer, number, common name, chromaticity under
/// a 3200 K source, and transmission (relative luminance).
pub struct GelSwatch {
    pub manufacturer: &'static str,
    pub number: &'static str,
    pub name: &'static str,
    pub x: f32,
    pub y: f32,
    pub transmission: f32,
}

/// The gel table. Values are approximate chromaticities read from the
/// manufacturers' published swatch data under tungsten, close enough to
/// pick the right emitters; a colourimeter would refine them.
// r[impl color.gel] - the swatch table a reference resolves through
pub const GELS: &[GelSwatch] = &[
    GelSwatch {
        manufacturer: "Lee",
        number: "201",
        name: "Full C.T. Blue",
        x: 0.313,
        y: 0.329,
        transmission: 0.35,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "202",
        name: "Half C.T. Blue",
        x: 0.365,
        y: 0.365,
        transmission: 0.55,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "204",
        name: "Full C.T. Orange",
        x: 0.520,
        y: 0.410,
        transmission: 0.55,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "205",
        name: "Half C.T. Orange",
        x: 0.470,
        y: 0.410,
        transmission: 0.70,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "106",
        name: "Primary Red",
        x: 0.690,
        y: 0.305,
        transmission: 0.06,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "101",
        name: "Yellow",
        x: 0.510,
        y: 0.470,
        transmission: 0.80,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "158",
        name: "Deep Orange",
        x: 0.610,
        y: 0.375,
        transmission: 0.25,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "139",
        name: "Primary Green",
        x: 0.280,
        y: 0.590,
        transmission: 0.12,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "116",
        name: "Medium Blue-Green",
        x: 0.210,
        y: 0.400,
        transmission: 0.20,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "119",
        name: "Dark Blue",
        x: 0.155,
        y: 0.090,
        transmission: 0.02,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "181",
        name: "Congo Blue",
        x: 0.170,
        y: 0.045,
        transmission: 0.01,
    },
    GelSwatch {
        manufacturer: "Lee",
        number: "126",
        name: "Mauve",
        x: 0.380,
        y: 0.190,
        transmission: 0.08,
    },
    GelSwatch {
        manufacturer: "Rosco",
        number: "02",
        name: "Bastard Amber",
        x: 0.480,
        y: 0.400,
        transmission: 0.62,
    },
    GelSwatch {
        manufacturer: "Rosco",
        number: "26",
        name: "Light Red",
        x: 0.660,
        y: 0.330,
        transmission: 0.10,
    },
    GelSwatch {
        manufacturer: "Rosco",
        number: "80",
        name: "Primary Blue",
        x: 0.160,
        y: 0.100,
        transmission: 0.02,
    },
    GelSwatch {
        manufacturer: "Rosco",
        number: "3202",
        name: "Full Blue (CTB)",
        x: 0.313,
        y: 0.329,
        transmission: 0.36,
    },
];

/// Looks a gel up, case-insensitive on the manufacturer, tolerant of a
/// leading `R`/`L` on the number (`"R80"`, `"L201"`).
pub fn gel(manufacturer: &str, number: &str) -> Option<&'static GelSwatch> {
    let number = number
        .trim()
        .trim_start_matches(['R', 'r', 'L', 'l'])
        .trim_start_matches('0');
    let number = if number.is_empty() { "0" } else { number };
    GELS.iter().find(|g| {
        g.manufacturer.eq_ignore_ascii_case(manufacturer.trim())
            && g.number.trim_start_matches('0') == number
    })
}

// --- Intent --------------------------------------------------------------

impl Intent {
    /// The intent as chromaticity plus luminance. A gel that is not in
    /// the table yields `None` — `r[color.unresolved-is-visible]` wants
    /// that reported, not guessed.
    pub fn xyy(&self) -> Option<Xyy> {
        Some(match self {
            Intent::Rgb(rgb) => rgb.to_xyy(),
            Intent::Xy { x, y, luminance } => Xyy {
                x: *x,
                y: *y,
                luminance: *luminance,
            },
            Intent::Cct { kelvin, tint } => {
                let (x, y) = cct_to_xy(*kelvin, *tint);
                Xyy {
                    x,
                    y,
                    luminance: 1.0,
                }
            }
            Intent::Gel {
                manufacturer,
                number,
            } => {
                // Transmission stays on the swatch; on an LED fixture a
                // gel is a colour, not a loss.
                let g = gel(manufacturer, number)?;
                Xyy {
                    x: g.x,
                    y: g.y,
                    luminance: 1.0,
                }
            }
        })
    }

    /// How bright, fixture-relative, `0..=1`: an RGB triple's peak
    /// channel (so `(1,0,0)` is red at full), an xy's luminance, and
    /// full for a colour temperature or a gel.
    pub fn brightness(&self) -> f32 {
        match self {
            Intent::Rgb(rgb) => rgb.red.max(rgb.green).max(rgb.blue),
            Intent::Xy { luminance, .. } => *luminance,
            Intent::Cct { .. } | Intent::Gel { .. } => 1.0,
        }
    }

    /// The RGB triple that stands in for this intent on a fixture with
    /// no emitter data. A white intent (CCT) renders at full brightness
    /// rather than at the locus point's raw luminance.
    // r[impl color.emitter-solve] - fixtures without emitter data fall back to RGB
    pub fn rgb(&self) -> Option<Rgb> {
        Some(match self {
            Intent::Rgb(rgb) => *rgb,
            Intent::Cct { .. } => {
                let xyy = self.xyy()?;
                let rgb = xyy.to_rgb();
                let max = rgb.red.max(rgb.green).max(rgb.blue).max(1e-6);
                Rgb::new(rgb.red / max, rgb.green / max, rgb.blue / max)
            }
            _ => self.xyy()?.to_rgb(),
        })
    }
}

// --- Colour spaces --------------------------------------------------------

/// A colour space an RGB-shaped triple can be read in. The intent a
/// preset stores is space-independent; this is how a triple that came
/// from a fixture type's GDTF space, a Rec.2020 media file or a raw xy
/// is given its real meaning before it is solved.
// r[impl color.spaces]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorSpace {
    /// Linear sRGB / Rec.709 primaries, D65 white.
    Srgb,
    /// ITU-R BT.2020 primaries, D65 white — the wide gamut a laser or
    /// a saturated LED reaches and sRGB cannot name.
    Rec2020,
    /// The triple *is* CIE xyY: `(x, y, Y)`, no primaries at all.
    Xy,
}

impl ColorSpace {
    /// Linear XYZ tristimulus of a triple read in this space.
    // r[impl color.spaces]
    pub fn to_xyz(self, triple: [f32; 3]) -> Xyz {
        let [a, b, c] = triple;
        match self {
            ColorSpace::Srgb => Rgb::new(a, b, c).to_xyz(),
            ColorSpace::Rec2020 => Xyz {
                x: 0.636_958 * a + 0.1446169 * b + 0.168_881 * c,
                y: 0.2627002 * a + 0.6779981 * b + 0.0593017 * c,
                z: 0.0000000 * a + 0.0280727 * b + 1.0609851 * c,
            },
            ColorSpace::Xy => Xyy {
                x: a,
                y: b,
                luminance: c,
            }
            .to_xyz(),
        }
    }

    /// A triple in this space for an XYZ — not clamped, so a colour
    /// outside this space's gamut comes back with a channel below zero
    /// or above one, and the caller can see that it did.
    // r[impl color.spaces]
    pub fn from_xyz(self, xyz: Xyz) -> [f32; 3] {
        match self {
            ColorSpace::Srgb => {
                let rgb = xyz.to_rgb();
                [rgb.red, rgb.green, rgb.blue]
            }
            ColorSpace::Rec2020 => [
                1.7166512 * xyz.x - 0.3556708 * xyz.y - 0.2533663 * xyz.z,
                -0.6666844 * xyz.x + 1.6164812 * xyz.y + 0.0157685 * xyz.z,
                0.0176399 * xyz.x - 0.0427706 * xyz.y + 0.9421031 * xyz.z,
            ],
            ColorSpace::Xy => {
                let xyy = xyz.to_xyy();
                [xyy.x, xyy.y, xyy.luminance]
            }
        }
    }

    /// Whether a triple in this space is inside its gamut: every
    /// channel in `0..=1` (for `Xy`, a chromaticity inside the unit
    /// triangle with a luminance in `0..=1`).
    pub fn contains(self, triple: [f32; 3]) -> bool {
        const EPS: f32 = 1e-4;
        let [a, b, c] = triple;
        match self {
            ColorSpace::Xy => {
                a >= -EPS && b >= -EPS && a + b <= 1.0 + EPS && (-EPS..=1.0 + EPS).contains(&c)
            }
            _ => [a, b, c].iter().all(|v| (-EPS..=1.0 + EPS).contains(v)),
        }
    }
}

impl Intent {
    /// A triple in `space` that reads as this intent, when its intent
    /// converts (a gel not in the table yields `None`). Space-independent
    /// intent in, a space's numbers out — the meeting point with a
    /// fixture type's declared colour space. Not clamped.
    // r[impl color.spaces]
    pub fn in_space(&self, space: ColorSpace) -> Option<[f32; 3]> {
        Some(space.from_xyz(self.xyy()?.to_xyz()))
    }

    /// An intent from a triple read in `space`: sRGB stays an `Rgb`
    /// (the form every older file has), the rest become `Xy` so the
    /// meaning survives outside sRGB's gamut.
    // r[impl color.spaces]
    pub fn from_space(space: ColorSpace, triple: [f32; 3]) -> Intent {
        match space {
            ColorSpace::Srgb => Intent::Rgb(Rgb::new(triple[0], triple[1], triple[2])),
            _ => {
                let xyy = space.to_xyz(triple).to_xyy();
                Intent::Xy {
                    x: xyy.x,
                    y: xyy.y,
                    luminance: xyy.luminance,
                }
            }
        }
    }

    /// This intent at luminance `luminance` (CIE `Y`): the same
    /// chromaticity, brighter or darker. Yields `Xy` unless nothing had
    /// to change.
    pub fn at_luminance(&self, luminance: f32) -> Option<Intent> {
        let xyy = self.xyy()?;
        if (xyy.luminance - luminance).abs() < 1e-6 {
            return Some(self.clone());
        }
        Some(Intent::Xy {
            x: xyy.x,
            y: xyy.y,
            luminance,
        })
    }
}

/// The same list of colours at one luminance — the *lowest* CIE `Y`
/// among them — so a chase that walks them does not pump: red at full
/// is a fifth the luminance of green at full, and a chase written as
/// three saturated RGB triples reads as a throb of brightness unless
/// every step is held to the dimmest one. A colour that cannot convert
/// (an unknown gel) is passed through unchanged; a list at zero
/// luminance is unchanged.
// r[impl color.spaces] - constant brightness across a hue change
pub fn constant_brightness(intents: &[Intent]) -> Vec<Intent> {
    let floor = intents
        .iter()
        .filter_map(|i| i.xyy())
        .map(|c| c.luminance)
        .filter(|y| *y > 0.0)
        .fold(f32::INFINITY, f32::min);
    if !floor.is_finite() {
        return intents.to_vec();
    }
    intents
        .iter()
        .map(|i| i.at_luminance(floor).unwrap_or_else(|| i.clone()))
        .collect()
}

// --- Mix or wheel -----------------------------------------------------------

/// How a fixture type that has both a colour wheel and colour mixing
/// reaches a colour preset. Set per fixture type; the renderer applies
/// it. A type with only one of the two has no choice to make.
// r[impl color.mix-or-wheel] - the preference, settable per fixture type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorPreference {
    /// Mix the colour on the emitters; the wheel stays open.
    #[default]
    Mix,
    /// Take the nearest wheel slot; the mixing stays white.
    Wheel,
}

/// The wheel slot whose colour is nearest `intent`, by CIE 1931 xy
/// distance — how a gel-only spot joins in "Congo Blue" by taking its
/// nearest gel. `None` for an empty wheel, or an intent that cannot
/// convert. Ties go to the earlier slot; a slot whose own intent
/// cannot convert is skipped.
// r[impl color.mix-or-wheel] - the nearest slot for a wheel-only fixture
pub fn nearest_wheel_slot(intent: &Intent, slots: &[(u8, Intent)]) -> Option<u8> {
    let want = intent.xyy()?;
    let mut best: Option<(u8, f32)> = None;
    for (slot, colour) in slots {
        let Some(c) = colour.xyy() else { continue };
        let d = (c.x - want.x).powi(2) + (c.y - want.y).powi(2);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((*slot, d));
        }
    }
    best.map(|(slot, _)| slot)
}

// --- Solve ---------------------------------------------------------------

/// The default `quality` when a preset does not say — MA3's Q fader at
/// its middle: some preference for the matching emitters, still a mix.
pub const DEFAULT_QUALITY: f32 = 0.5;

/// Emitter levels (`0..=1`, one per emitter, in the order given) that
/// reach `intent` on a fixture with these emitters.
///
/// Two steps. First the *chromaticity*: box-constrained least squares in
/// XYZ by projected gradient descent — minimise `|M w - target|²` with
/// `M`'s columns the emitters' full-output XYZ and `w ∈ [0,1]^n` — run at
/// a luminance low enough that nothing clips, so the hue is exact.
/// `quality` adds a tie-breaker there: at 1 an L1 term (fewest emitters,
/// the narrow-band ones that match), at 0 a term pulling the levels
/// toward each other (broadband, every emitter contributing). The fit
/// dominates either way: colour first, mixing style second.
///
/// Then the *brightness*, which is fixture-relative the way every
/// console's colour is: the mix is scaled so that a full-brightness
/// intent puts its limiting emitter at full. `Rgb(1,0,0)` is the red LED
/// at 100 % (not the 65 % that sRGB's dimmer red primary would be),
/// `Rgb(0.5,0.5,0.5)` is half of the brightest white this fixture can
/// make, and a 3200 K CCT is the brightest 3200 K this fixture can make.
///
/// With no emitters the answer is the intent's RGB triple, so a caller
/// can always index `[0..3]`.
// r[impl color.emitter-solve] - intent to levels against the fixture's own emitters
// r[impl color.quality] - 1 favours few narrow emitters, 0 spreads across all
pub fn solve(intent: &Intent, emitters: &[Emitter], quality: f32) -> Vec<f32> {
    let Some(target) = intent.xyy() else {
        return Vec::new();
    };
    if emitters.is_empty() {
        let rgb = intent.rgb().unwrap_or_default();
        return vec![rgb.red, rgb.green, rgb.blue];
    }
    let brightness = intent.brightness().clamp(0.0, 1.0);
    if brightness <= 0.0 {
        return vec![0.0; emitters.len()];
    }
    let mut w = solve_chromaticity(target, emitters, quality.clamp(0.0, 1.0));
    let peak = w.iter().copied().fold(0.0f32, f32::max);
    if peak > 0.0 {
        for v in &mut w {
            *v = (*v * brightness / peak).clamp(0.0, 1.0);
        }
    }
    w
}

/// Solve luminance: a small fraction of the dimmest emitter's full
/// output, so no level reaches its bound and the chromaticity is exact.
const PROBE_FRACTION: f32 = 0.05;

/// The mix (unscaled, every level well inside `0..1`) that has `target`'s
/// chromaticity. See `solve` for the objective.
fn solve_chromaticity(target: Xyy, emitters: &[Emitter], q: f32) -> Vec<f32> {
    let n = emitters.len();
    let peak = emitters
        .iter()
        .map(|e| e.max_lumens)
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let dimmest = emitters
        .iter()
        .map(|e| e.max_lumens / peak)
        .fold(f32::INFINITY, f32::min)
        .max(1e-3);
    let cols: Vec<[f32; 3]> = emitters
        .iter()
        .map(|e| {
            let xyz = Xyy {
                x: e.x,
                y: e.y,
                luminance: e.max_lumens / peak,
            }
            .to_xyz();
            [xyz.x, xyz.y, xyz.z]
        })
        .collect();
    let probe = PROBE_FRACTION * dimmest;
    let t = Xyy {
        luminance: probe,
        ..target
    }
    .to_xyz();
    let t = [t.x, t.y, t.z];

    // Lipschitz-ish step from the largest column norm.
    let col_norm = cols
        .iter()
        .map(|c| c[0] * c[0] + c[1] * c[1] + c[2] * c[2])
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let step = 0.5 / (col_norm * n as f32);
    // Tie-breakers, scaled with the probe so they stay a tie-breaker.
    let narrow = 0.02 * q * probe;
    let broad = 0.02 * (1.0 - q);

    let mut w = vec![probe; n];
    for _ in 0..4000 {
        let mut r = [0.0f32; 3];
        for (i, c) in cols.iter().enumerate() {
            r[0] += c[0] * w[i];
            r[1] += c[1] * w[i];
            r[2] += c[2] * w[i];
        }
        r[0] -= t[0];
        r[1] -= t[1];
        r[2] -= t[2];
        let mean = w.iter().sum::<f32>() / n as f32;
        let mut max_delta = 0.0f32;
        for (i, c) in cols.iter().enumerate() {
            let grad_fit = 2.0 * (c[0] * r[0] + c[1] * r[1] + c[2] * r[2]);
            let grad = grad_fit + narrow + 2.0 * broad * (w[i] - mean);
            let next = (w[i] - step * grad).clamp(0.0, 1.0);
            max_delta = max_delta.max((next - w[i]).abs());
            w[i] = next;
        }
        if max_delta < 1e-7 * probe {
            break;
        }
    }
    // Snap the dust the L1 term leaves behind.
    let floor = 1e-3 * probe;
    for v in &mut w {
        if *v < floor {
            *v = 0.0;
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgbw() -> Vec<Emitter> {
        vec![
            emitter("Red", 0.69, 0.31, 0.25),
            emitter("Green", 0.20, 0.70, 0.60),
            emitter("Blue", 0.14, 0.06, 0.10),
            emitter("White", 0.32, 0.34, 1.0),
        ]
    }

    fn rgba() -> Vec<Emitter> {
        vec![
            emitter("Red", 0.69, 0.31, 0.25),
            emitter("Green", 0.20, 0.70, 0.60),
            emitter("Blue", 0.14, 0.06, 0.10),
            emitter("Amber", 0.58, 0.41, 0.40),
        ]
    }

    #[test]
    /// r[verify color.model] - RGB and HSB round-trip within float precision
    fn rgb_and_hsb_interconvert_without_loss() {
        for rgb in [
            Rgb::new(1.0, 0.5, 0.1),
            Rgb::new(0.1, 0.2, 0.9),
            Rgb::new(0.3, 0.3, 0.3),
            Rgb::new(0.0, 1.0, 0.0),
        ] {
            let back = Rgb::from_hsb(rgb.to_hsb());
            assert!((back.red - rgb.red).abs() < 1e-5, "{rgb:?} -> {back:?}");
            assert!((back.green - rgb.green).abs() < 1e-5);
            assert!((back.blue - rgb.blue).abs() < 1e-5);
        }
    }

    #[test]
    /// r[verify color.intent] - xyY and RGB are the same colour both ways
    fn rgb_to_xyy_and_back_is_identity() {
        let rgb = Rgb::new(0.8, 0.3, 0.1);
        let back = rgb.to_xyy().to_rgb();
        assert!((back.red - 0.8).abs() < 1e-4);
        assert!((back.green - 0.3).abs() < 1e-4);
        assert!((back.blue - 0.1).abs() < 1e-4);
        let white = Rgb::WHITE.to_xyy();
        assert!((white.x - 0.3127).abs() < 1e-3, "sRGB white is D65");
        assert!((white.y - 0.3290).abs() < 1e-3);
    }

    #[test]
    /// r[verify color.cct] - 6500 K lands on D65, 3200 K is warmer
    fn cct_lands_on_the_planckian_locus() {
        let (x, y) = cct_to_xy(6500.0, 0.0);
        assert!((x - 0.3135).abs() < 0.003, "{x}");
        assert!((y - 0.3237).abs() < 0.003, "{y}");
        let (x, y) = cct_to_xy(3200.0, 0.0);
        assert!((x - 0.4234).abs() < 0.003, "{x}");
        assert!((y - 0.3990).abs() < 0.003, "{y}");
        let (_, y_green) = cct_to_xy(3200.0, 1.0);
        assert!(y_green > y, "positive tint goes green");
    }

    #[test]
    /// r[verify color.gel] - a gel resolves through the table, keeping its reference
    fn a_gel_resolves_and_the_reference_survives() {
        let g = Intent::Gel {
            manufacturer: "lee".into(),
            number: "L201".into(),
        };
        let xyy = g.xyy().expect("Lee 201 is in the table");
        assert!(xyy.x < 0.32, "full CTB is bluish-white");
        assert_eq!(gel("Lee", "201").unwrap().name, "Full C.T. Blue");
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains("201"), "{json}");
        assert!(
            Intent::Gel {
                manufacturer: "Acme".into(),
                number: "1".into()
            }
            .xyy()
            .is_none()
        );
    }

    #[test]
    /// r[verify color.cct] - a fixture with a white emitter uses it for a white
    /// r[verify color.emitter-solve]
    fn warm_white_on_rgbw_uses_the_white_emitter() {
        let w = solve(
            &Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0,
            },
            &rgbw(),
            DEFAULT_QUALITY,
        );
        assert_eq!(w.len(), 4);
        // Warming a ~5600 K white LED to 3200 K takes a lot of red LED
        // (a quarter of the white's output at full), so the *level* on
        // red is the higher one; the *light* still comes mostly from
        // the white.
        assert!(w[3] > 0.3, "white is lit: {w:?}");
        assert!(w[3] * 1.0 > w[0] * 0.25, "white carries the light: {w:?}");
        assert!(w[0] > w[2], "3200 K needs more red than blue: {w:?}");
        assert!(
            (w.iter().copied().fold(0.0f32, f32::max) - 1.0).abs() < 1e-4,
            "at full"
        );
    }

    #[test]
    /// r[verify color.emitter-solve] - a red intent on RGBA is red first
    fn red_on_rgba_is_red_dominant() {
        let w = solve(&Intent::Rgb(Rgb::new(1.0, 0.0, 0.0)), &rgba(), 0.8);
        assert!((w[0] - 1.0).abs() < 1e-4, "red at full: {w:?}");
        assert!(w[1] < 0.05 && w[2] < 0.05, "{w:?}");
        let half = solve(&Intent::Rgb(Rgb::new(0.5, 0.0, 0.0)), &rgba(), 0.8);
        assert!((half[0] - 0.5).abs() < 1e-3, "half red is half: {half:?}");
        assert!(w[0] > w[3] && w[0] > w[1] && w[0] > w[2], "{w:?}");
    }

    #[test]
    /// r[verify color.emitter-solve] - no emitter data, the RGB triple
    fn no_emitters_falls_back_to_rgb() {
        let w = solve(&Intent::Rgb(Rgb::new(0.2, 0.4, 0.6)), &[], 0.5);
        assert_eq!(w, vec![0.2, 0.4, 0.6]);
        let w = solve(
            &Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0,
            },
            &[],
            0.5,
        );
        assert_eq!(w.len(), 3);
        assert!((w[0] - 1.0).abs() < 1e-4, "warm white peaks at red: {w:?}");
        assert!(w[2] < w[1] && w[1] < w[0], "{w:?}");
    }

    #[test]
    /// r[verify color.quality] - quality 1 narrows, 0 spreads
    fn quality_steers_narrow_against_broad() {
        let target = Intent::Rgb(Rgb::new(1.0, 1.0, 1.0));
        let narrow = solve(&target, &rgbw(), 1.0);
        let broad = solve(&target, &rgbw(), 0.0);
        let lit = |w: &[f32]| w.iter().filter(|v| **v > 0.05).count();
        assert!(lit(&narrow) <= lit(&broad), "{narrow:?} vs {broad:?}");
        let spread = |w: &[f32]| {
            let mean = w.iter().sum::<f32>() / w.len() as f32;
            w.iter().map(|v| (v - mean).powi(2)).sum::<f32>()
        };
        assert!(spread(&broad) <= spread(&narrow) + 1e-6);
    }

    // --- Colour spaces and mix-or-wheel ---

    /// r[verify color.spaces]
    #[test]
    fn srgb_round_trips_through_xyz() {
        for triple in [
            [1.0, 0.0, 0.0],
            [0.2, 0.7, 0.4],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.3],
        ] {
            let back = ColorSpace::Srgb.from_xyz(ColorSpace::Srgb.to_xyz(triple));
            for (a, b) in triple.iter().zip(&back) {
                assert!((a - b).abs() < 1e-4, "{triple:?} -> {back:?}");
            }
            let back = ColorSpace::Rec2020.from_xyz(ColorSpace::Rec2020.to_xyz(triple));
            for (a, b) in triple.iter().zip(&back) {
                assert!((a - b).abs() < 1e-4, "rec2020 {triple:?} -> {back:?}");
            }
        }
        // White is white in both: D65 at Y = 1.
        let w = ColorSpace::Rec2020.to_xyz([1.0, 1.0, 1.0]);
        let s = ColorSpace::Srgb.to_xyz([1.0, 1.0, 1.0]);
        assert!((w.x - s.x).abs() < 1e-3 && (w.y - 1.0).abs() < 1e-3 && (w.z - s.z).abs() < 1e-3);
        // Xy is xyY as it stands.
        let xy = ColorSpace::Xy.from_xyz(ColorSpace::Xy.to_xyz([0.3, 0.4, 0.5]));
        assert!(
            (xy[0] - 0.3).abs() < 1e-5 && (xy[1] - 0.4).abs() < 1e-5 && (xy[2] - 0.5).abs() < 1e-5
        );
    }

    /// r[verify color.spaces]
    #[test]
    fn rec2020_red_is_outside_srgb() {
        let red = ColorSpace::Rec2020.to_xyz([1.0, 0.0, 0.0]);
        let in_srgb = ColorSpace::Srgb.from_xyz(red);
        assert!(!ColorSpace::Srgb.contains(in_srgb), "{in_srgb:?}");
        assert!(in_srgb[1] < 0.0, "green goes negative: {in_srgb:?}");
        // While sRGB red sits inside Rec.2020.
        let srgb_red = ColorSpace::Srgb.to_xyz([1.0, 0.0, 0.0]);
        assert!(ColorSpace::Rec2020.contains(ColorSpace::Rec2020.from_xyz(srgb_red)));
        // An intent reads out in either space, and one made from a
        // Rec.2020 triple keeps its chromaticity as xy.
        let intent = Intent::from_space(ColorSpace::Rec2020, [1.0, 0.0, 0.0]);
        assert!(matches!(intent, Intent::Xy { .. }));
        let back = intent.in_space(ColorSpace::Rec2020).unwrap();
        assert!((back[0] - 1.0).abs() < 1e-3 && back[1].abs() < 1e-3 && back[2].abs() < 1e-3);
        assert!(!ColorSpace::Srgb.contains(intent.in_space(ColorSpace::Srgb).unwrap()));
    }

    /// r[verify color.spaces] - constant brightness
    #[test]
    fn constant_brightness_equalises_luminance() {
        let chase = vec![
            Intent::Rgb(Rgb::new(1.0, 0.0, 0.0)),
            Intent::Rgb(Rgb::new(0.0, 1.0, 0.0)),
            Intent::Rgb(Rgb::new(0.0, 0.0, 1.0)),
        ];
        let ys: Vec<f32> = chase.iter().map(|i| i.xyy().unwrap().luminance).collect();
        assert!(ys[1] > ys[0] * 3.0, "green pumps over red: {ys:?}");
        let even = constant_brightness(&chase);
        let ys: Vec<f32> = even.iter().map(|i| i.xyy().unwrap().luminance).collect();
        let floor = Rgb::new(0.0, 0.0, 1.0).to_xyz().y;
        assert!(ys.iter().all(|y| (y - floor).abs() < 1e-5), "{ys:?}");
        // Chromaticity is untouched.
        for (a, b) in chase.iter().zip(&even) {
            let (a, b) = (a.xyy().unwrap(), b.xyy().unwrap());
            assert!((a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5);
        }
        // The dimmest one is returned as it was.
        assert_eq!(even[2], chase[2]);
        // Nothing to convert, nothing changes.
        let unknown = vec![Intent::Gel {
            manufacturer: "Nobody".into(),
            number: "0".into(),
        }];
        assert_eq!(constant_brightness(&unknown), unknown);
    }

    /// r[verify color.mix-or-wheel]
    #[test]
    fn the_nearest_wheel_slot_is_by_xy_distance() {
        let wheel = vec![
            (0, Intent::Rgb(Rgb::WHITE)),
            (1, Intent::Rgb(Rgb::new(1.0, 0.0, 0.0))),
            (2, Intent::Rgb(Rgb::new(0.0, 1.0, 0.0))),
            (3, Intent::Rgb(Rgb::new(0.0, 0.0, 1.0))),
            (
                4,
                Intent::Gel {
                    manufacturer: "Nobody".into(),
                    number: "0".into(),
                },
            ),
        ];
        assert_eq!(
            nearest_wheel_slot(&Intent::Rgb(Rgb::new(0.9, 0.1, 0.1)), &wheel),
            Some(1)
        );
        assert_eq!(
            nearest_wheel_slot(
                &Intent::Cct {
                    kelvin: 6500.0,
                    tint: 0.0
                },
                &wheel
            ),
            Some(0)
        );
        // Congo Blue lands on the blue slot; luminance plays no part.
        assert_eq!(
            nearest_wheel_slot(
                &Intent::Xy {
                    x: 0.16,
                    y: 0.05,
                    luminance: 0.1
                },
                &wheel
            ),
            Some(3)
        );
        assert_eq!(nearest_wheel_slot(&Intent::Rgb(Rgb::WHITE), &[]), None);
        assert_eq!(ColorPreference::default(), ColorPreference::Mix);
        assert_eq!(
            serde_json::to_string(&ColorPreference::Wheel).unwrap(),
            r#""wheel""#
        );
    }
}
