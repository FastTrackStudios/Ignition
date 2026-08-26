//! Every library effect resolves to at least one fixture at Norco, and
//! at Riverside — the whole point of writing them against roles.

use ignition_core::{Show, SpeedMasters};
use ignition_viz::venue::Venue;

/// r[verify effects.library.roles-only]
/// r[verify effects.library.missing-role-is-empty]
#[test]
fn every_library_effect_cooks_somewhere() {
    let speeds = SpeedMasters::from([("Song".to_string(), 120.0f32), ("Tap".to_string(), 120.0)]);
    let mut never = Vec::new();
    for name in ignition_core::effects::library().keys() {
        let mut hit = false;
        for venue in ["../../data/venues/norco", "../../data/venues/riverside"] {
            let Ok(venue) = Venue::load(venue) else {
                return;
            };
            let groups = venue.groups();
            let rig = venue.rig();
            let show = Show {
                groups: &groups,
                palettes: &venue.palettes,
                rig: &rig,
                speeds: &speeds,
                roles: &venue.profile,
                ..Show::new(&groups, &rig)
            };
            let recipe = &ignition_core::effects::library()[name];
            // A role this room does not bind is the graceful case the
            // spec allows; the defect is a *bound* role cooking to
            // nothing.
            let unbound = ignition_core::selection::unresolved_names_with(
                &recipe.target,
                &groups,
                &rig,
                &venue.profile,
            );
            if !unbound.is_empty() {
                hit = true;
                continue;
            }
            // A one-shot at its first step, a loop mid-cycle. A pattern in
            // metres emits focus deltas rather than attribute values.
            let full = ignition_core::recipe::expand_recipe_full(recipe, &show, 0.05);
            if !full.emits.is_empty() || !full.focus_deltas.is_empty() {
                hit = true;
            }
        }
        if !hit {
            never.push(name.clone());
        }
    }
    assert!(
        never.is_empty(),
        "effects that cook to nothing at both venues: {never:?}"
    );
}
