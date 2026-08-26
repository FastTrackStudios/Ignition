//! What the live renderer spends its frame on, and what it declines to.
//!
//! Every spill light *could* cast a shadow map and carve a volumetric
//! shaft, and on a rig of seventy fixtures that is seventy shadow passes
//! and a fog raymarch that samples seventy shadow maps at every step of
//! every pixel — the frame the studio measured at two and a half per
//! second. The picture does not need it: a par's cone in the haze reads
//! the same with or without a person's shadow cut out of it, and it is
//! the nine moving heads whose hard shafts are what shadows are *for*.
//!
//! So the budget is a count, spent on the brightest per-direction
//! fixtures first — see `r[viz.performance-budget]`.

/// How many spill lights may carry a shadow map. Bevy renders one shadow
/// view per shadowed spot per frame, and the fog pass samples that map
/// at every raymarch step: both costs scale with this number and
/// nothing else in the rig. Twelve covers every moving head at Norco
/// (nine) and Riverside with room for a couple of profile spots.
pub const SHADOW_BUDGET: usize = 12;

/// How many spill lights may be volumetric — lit into the haze by the
/// fog pass. Every raymarch step of every pixel loops over the
/// volumetric lights whose cones reach that cluster, so this number
/// multiplies the cost of the fog directly. `IGNITION_VOLUMETRIC_BUDGET`
/// overrides it, for comparing on a given GPU without a rebuild.
pub const VOLUMETRIC_BUDGET: usize = 16;

/// `SHADOW_BUDGET`, or `IGNITION_SHADOW_BUDGET`'s say — the same dial
/// for comparing on a given GPU.
pub fn shadow_budget_setting() -> usize {
    std::env::var("IGNITION_SHADOW_BUDGET")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(SHADOW_BUDGET)
}

/// Whether a shaft-cutter over the volumetric budget — a par, at Norco —
/// is drawn in the air as the hand-drawn additive cone instead. Off by
/// default: forty-eight translucent cones with a noise shader cost
/// close to two milliseconds of GPU at 5120x1440, which is the margin
/// the budget leaves, for cones the par's floor pool already implies.
/// `IGNITION_PAR_CONES=1` turns them on.
pub fn par_cones() -> bool {
    std::env::var("IGNITION_PAR_CONES").is_ok_and(|v| v == "1")
}

/// `VOLUMETRIC_BUDGET`, or the environment's say.
pub fn volumetric_budget() -> usize {
    std::env::var("IGNITION_VOLUMETRIC_BUDGET")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(VOLUMETRIC_BUDGET)
}

/// Which of `candidates` get a shadow map: the `budget` brightest by
/// peak candela among those that cut a shaft at all. Returns one flag
/// per candidate, in order.
///
/// Brightness per direction rather than "is a mover" because it is the
/// same rule that decides whether a fixture cuts a shaft, and the
/// fixtures whose shadows matter are exactly the ones whose shafts are
/// hard enough to be cut: a 1.7-degree beam at eight million candela,
/// a gobo mover at forty thousand. A 36 W par at six thousand is a wash
/// on the floor, and nobody sees the silhouette it fails to throw.
// r[impl viz.performance-budget] - shadows go to the brightest shaft-cutters
pub fn shadow_budget(candidates: &[ShadowCandidate], budget: usize) -> Vec<bool> {
    let mut ranked: Vec<(usize, f32)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.cuts_a_shaft)
        .map(|(i, c)| (i, c.candela))
        .collect();
    // Brightest first; a tie keeps patch order, so the answer is stable
    // between runs and between venues that share a fixture type.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    // A fixture type is in or out as a whole. Forty-eight identical
    // pars with three of them cutting a shaft and forty-five not would
    // read as three broken pars, not as a budget — so if the cut would
    // fall inside a run of equal brightness, the whole run stays out.
    let mut take = budget.min(ranked.len());
    while take > 0 && take < ranked.len() && ranked[take - 1].1 == ranked[take].1 {
        take -= 1;
    }
    let mut out = vec![false; candidates.len()];
    for (i, _) in ranked.into_iter().take(take) {
        out[i] = true;
    }
    out
}

/// One spill light asking for a shadow map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowCandidate {
    /// Peak candela on axis — `fixture_profile::peak_candela`.
    pub candela: f32,
    /// Whether it is bright enough per-direction to be a volumetric
    /// light at all (`SHAFT_CANDELA_THRESHOLD`). A light that lights
    /// no air has no shaft for a shadow to cut.
    pub cuts_a_shaft: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(candela: f32, cuts_a_shaft: bool) -> ShadowCandidate {
        ShadowCandidate {
            candela,
            cuts_a_shaft,
        }
    }

    /// Norco in miniature: pars at 6,400 cd, gobo movers at 39,000, the
    /// Betopper beams past 8,000,000, strips that cut no shaft. The
    /// movers and beams get the maps; the pars, though above the shaft
    /// threshold, do not.
    /// r[verify viz.performance-budget]
    #[test]
    fn the_budget_goes_to_the_brightest_shaft_cutters_and_never_to_a_wash() {
        let mut rig = vec![c(6_400.0, true); 48];
        rig.extend(std::iter::repeat_n(c(39_000.0, true), 4));
        rig.extend(std::iter::repeat_n(c(8_000_000.0, true), 5));
        rig.extend(std::iter::repeat_n(c(600.0, false), 8));
        let flags = shadow_budget(&rig, SHADOW_BUDGET);
        assert!(flags[..48].iter().all(|f| !f), "a par got a shadow map");
        assert!(flags[48..57].iter().all(|f| *f), "a mover went without");
        assert!(flags[57..].iter().all(|f| !f), "a strip got a shadow map");
        assert_eq!(flags.iter().filter(|f| **f).count(), 9);
    }

    /// The cut never splits a run of equal fixtures: with room for
    /// two of three identical lights, none of the three is taken.
    /// r[verify viz.performance-budget]
    #[test]
    fn a_type_is_in_or_out_as_a_whole() {
        let rig = vec![c(100.0, true), c(10.0, true), c(10.0, true), c(10.0, true)];
        assert_eq!(shadow_budget(&rig, 3), vec![true, false, false, false]);
        assert_eq!(shadow_budget(&rig, 4), vec![true, true, true, true]);
        // Sixteen volumetric slots at Norco: the nine heads and the
        // four SlimPARs fit; the forty-eight pars do not, and none of
        // them is taken.
        let mut rig = vec![c(6_400.0, true); 48];
        rig.extend(std::iter::repeat_n(c(8_400.0, true), 4));
        rig.extend(std::iter::repeat_n(c(39_000.0, true), 4));
        rig.extend(std::iter::repeat_n(c(8_000_000.0, true), 5));
        let flags = shadow_budget(&rig, VOLUMETRIC_BUDGET);
        assert_eq!(flags.iter().filter(|f| **f).count(), 13);
        assert!(flags[..48].iter().all(|f| !f));
    }

    /// Over budget, the brightest win and the count is exactly the
    /// budget; a fixture that cuts no shaft never counts, however
    /// bright it claims to be.
    #[test]
    fn over_budget_the_brightest_win_and_the_count_is_the_budget() {
        let rig = vec![
            c(10.0, true),
            c(50.0, true),
            c(9_999_999.0, false),
            c(30.0, true),
            c(40.0, true),
        ];
        assert_eq!(
            shadow_budget(&rig, 2),
            vec![false, true, false, false, true]
        );
        assert_eq!(shadow_budget(&rig, 0), vec![false; 5]);
    }
}
