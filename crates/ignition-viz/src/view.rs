//! Camera framing presets.
//!
//! These are the same auto-framing rules the pre-Bevy renderer used —
//! they take a venue's bounds and place an eye/target pair, so they work
//! for any venue rather than being tuned to Norco's numbers. What changed
//! is the output: a `Transform` plus a vertical field of view for Bevy's
//! `Camera3d`, instead of a hand-built view-projection matrix.
//!
//! **The world is Z-up**, not Bevy's usual Y-up. That is the convention
//! the venue data, GDTF, and the lighting industry all use, and changing
//! it would mean remapping every fixture pose, every room object and
//! every beam aim on the way in. Bevy has no problem with it — nothing in
//! the renderer assumes an up axis — but it does mean every `looking_at`
//! here passes `Vec3::Z` as the up vector, and a Bevy example pasted in
//! unchanged will be sideways.

use bevy::prelude::*;

/// The camera positions the CLI can ask for by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    /// Front of house: near the back of the audience at standing height,
    /// looking at the rig — how the operator actually sees the room.
    House,
    /// Mid-stage looking back into the house — the performer's-eye view.
    Stage,
    /// Straight down. Pair with excluding the ceiling or it renders the
    /// roof and nothing else.
    Top,
}

impl ViewPreset {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "house" => Some(Self::House),
            "stage" => Some(Self::Stage),
            "top" => Some(Self::Top),
            _ => None,
        }
    }

    pub fn fov_y_deg(self) -> f32 {
        match self {
            Self::House => 60.0,
            Self::Stage => 65.0,
            Self::Top => 45.0,
        }
    }

    /// Where to put the camera for this preset, given the venue's bounds.
    pub fn transform(self, min: Vec3, max: Vec3) -> Transform {
        let size = max - min;
        let center = (min + max) * 0.5;
        let (eye, target, up) = match self {
            Self::House => (
                Vec3::new(center.x + size.x * 0.15, min.y + size.y * 0.06, min.z + size.z * 0.28),
                Vec3::new(center.x, center.y, min.z + size.z * 0.55),
                Vec3::Z,
            ),
            Self::Stage => (
                Vec3::new(center.x, max.y - size.y * 0.35, min.z + size.z * 0.55),
                Vec3::new(center.x, min.y + size.y * 0.15, min.z + size.z * 0.22),
                Vec3::Z,
            ),
            Self::Top => {
                let extent = size.length().max(4.0);
                (
                    Vec3::new(center.x, center.y, max.z + extent * 0.6),
                    Vec3::new(center.x, center.y, min.z),
                    // Looking straight down Z, so Z cannot also be the up
                    // vector — the look-at would be degenerate.
                    Vec3::Y,
                )
            }
        };
        Transform::from_translation(eye).looking_at(target, up)
    }

    /// Far plane, sized to the room so a large venue does not clip.
    pub fn far(self, min: Vec3, max: Vec3) -> f32 {
        (max - min).length().max(4.0) * 4.0 + 10.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> (Vec3, Vec3) {
        (Vec3::new(-5.0, -10.0, 0.0), Vec3::new(5.0, 10.0, 4.0))
    }

    #[test]
    fn every_preset_name_round_trips() {
        for name in ["house", "stage", "top"] {
            assert!(ViewPreset::parse(name).is_some(), "{name}");
        }
        assert!(ViewPreset::parse("sideways").is_none());
    }

    #[test]
    fn the_house_camera_stands_in_the_room_and_faces_the_stage() {
        let (min, max) = bounds();
        let t = ViewPreset::House.transform(min, max);
        assert!(t.translation.z > min.z && t.translation.z < max.z, "{}", t.translation.z);
        // Stage is +y in this data, and the house camera sits at -y, so
        // it must be looking that way.
        assert!(t.forward().y > 0.0, "{:?}", t.forward());
    }

    #[test]
    fn the_top_camera_looks_straight_down() {
        let (min, max) = bounds();
        let t = ViewPreset::Top.transform(min, max);
        assert!(t.translation.z > max.z);
        assert!(t.forward().z < -0.99, "{:?}", t.forward());
    }
}
