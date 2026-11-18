//! Quality tiers, and the governor that moves between them.
//!
//! Every tier is free of grids, march steps and history; what a tier
//! changes is how finely the smooth terms are weighted along the ray and
//! how the gobo is put in the air. So no tier can produce blocks, and a
//! change of tier is a change of softness, not of artefacts.

/// How a gobo shows in the air.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GoboInAir {
    /// The beam is plain; the gobo only shows where it lands.
    None,
    /// One sample where the view ray passes closest to the beam's axis.
    Soft,
    /// The gobo averaged along the ray over this many prefiltered samples.
    /// It only scales the closed-form light, so it cannot make the beam
    /// itself noisy.
    Steps(u8),
}

/// A quality tier: how much each beam's pixel costs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BeamTier {
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

impl BeamTier {
    /// Every tier, cheapest first.
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    /// The parts the ray's span in a beam is cut into. The inverse square
    /// is exact on each; the profile, phase and extinction are taken at
    /// each part's middle.
    #[must_use]
    pub const fn pieces(self) -> u8 {
        match self {
            Self::Low => 4,
            Self::Medium => 6,
            Self::High => 8,
            Self::Ultra => 16,
        }
    }

    #[must_use]
    pub const fn gobo(self) -> GoboInAir {
        match self {
            Self::Low => GoboInAir::None,
            Self::Medium => GoboInAir::Soft,
            Self::High => GoboInAir::Steps(8),
            Self::Ultra => GoboInAir::Steps(16),
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Ultra => "ultra",
        }
    }

    /// The tier called `name`, as [`BeamTier::name`] spells it.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
    }
}

/// The size of the target the beams are summed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AirResolution {
    /// Half the view each way, brought up with a depth-aware filter. Beam
    /// edges and gobos soften; nothing else changes, because what is being
    /// filtered is smooth. Only the governor's last resort.
    Half,
    /// The view's own size.
    #[default]
    Full,
}

/// What the governor has settled on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Quality {
    // Resolution first: it is the more significant in the ordering, so
    // every half-resolution quality is below every full-resolution one.
    pub resolution: AirResolution,
    pub tier: BeamTier,
}

impl Quality {
    /// One step cheaper: down the tiers at full resolution, then to half
    /// resolution at the lowest tier. `None` at the bottom.
    #[must_use]
    pub const fn cheaper(self) -> Option<Self> {
        let tier = match (self.tier, self.resolution) {
            (BeamTier::Ultra, _) => BeamTier::High,
            (BeamTier::High, _) => BeamTier::Medium,
            (BeamTier::Medium, _) => BeamTier::Low,
            (BeamTier::Low, AirResolution::Full) => {
                return Some(Self {
                    resolution: AirResolution::Half,
                    tier: BeamTier::Low,
                });
            }
            (BeamTier::Low, AirResolution::Half) => return None,
        };
        Some(Self { tier, ..self })
    }

    /// One step better: back to full resolution first, then up the tiers.
    /// `None` at the top.
    #[must_use]
    pub const fn better(self) -> Option<Self> {
        if matches!(self.resolution, AirResolution::Half) {
            return Some(Self {
                resolution: AirResolution::Full,
                ..self
            });
        }
        let tier = match self.tier {
            BeamTier::Low => BeamTier::Medium,
            BeamTier::Medium => BeamTier::High,
            BeamTier::High => BeamTier::Ultra,
            BeamTier::Ultra => return None,
        };
        Some(Self { tier, ..self })
    }
}

/// Keeps the frame inside a budget by moving the beams' quality.
///
/// It is fed the measured time of each frame and answers with the quality
/// to render the next one at. It drops a step quickly when frames run
/// over, and climbs back slowly and only with headroom, so it does not
/// hunt between two steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Governor {
    /// The frame time to stay under, in milliseconds.
    pub budget_ms: f32,
    /// The best quality it may climb to.
    pub ceiling: Quality,
    quality: Quality,
    /// The smoothed frame time, in milliseconds.
    average_ms: f32,
    /// Frames since the quality last changed.
    settled: u16,
}

/// How much of each new frame time goes into the average.
const SMOOTHING: f32 = 0.1;
/// Over budget by this factor, for [`DROP_AFTER`] frames: drop a step.
const OVER: f32 = 1.08;
const DROP_AFTER: u16 = 20;
/// Under budget by this factor, for [`CLIMB_AFTER`] frames: climb a step.
const UNDER: f32 = 0.7;
const CLIMB_AFTER: u16 = 240;

impl Governor {
    /// A governor that starts at `ceiling` and stays under `budget_ms`.
    #[must_use]
    pub const fn new(budget_ms: f32, ceiling: Quality) -> Self {
        Self {
            budget_ms,
            ceiling,
            quality: ceiling,
            average_ms: budget_ms,
            settled: 0,
        }
    }

    /// The quality to render at now.
    #[must_use]
    pub const fn quality(&self) -> Quality {
        self.quality
    }

    /// The smoothed frame time it is steering by, in milliseconds.
    #[must_use]
    pub const fn average_ms(&self) -> f32 {
        self.average_ms
    }

    /// Takes one measured frame time and returns the quality for the next
    /// frame.
    pub fn observe(&mut self, frame_ms: f32) -> Quality {
        self.average_ms += (frame_ms - self.average_ms) * SMOOTHING;
        self.settled = self.settled.saturating_add(1);
        let next = if self.average_ms > self.budget_ms * OVER && self.settled >= DROP_AFTER {
            self.quality.cheaper()
        } else if self.average_ms < self.budget_ms * UNDER
            && self.settled >= CLIMB_AFTER
            && self.quality < self.ceiling
        {
            self.quality.better()
        } else {
            None
        };
        if let Some(next) = next {
            self.quality = next;
            self.settled = 0;
        }
        self.quality
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: Quality = Quality {
        resolution: AirResolution::Full,
        tier: BeamTier::Ultra,
    };

    fn run(governor: &mut Governor, frame_ms: f32, frames: u16) -> Quality {
        (0..frames).fold(governor.quality(), |_, _| governor.observe(frame_ms))
    }

    #[test]
    fn the_ladder_runs_from_ultra_down_to_half_resolution_and_back() {
        let mut down = vec![TOP];
        while let Some(next) = down.last().copied().and_then(Quality::cheaper) {
            down.push(next);
        }
        assert_eq!(down.len(), 5);
        assert_eq!(
            down.last().copied(),
            Some(Quality {
                resolution: AirResolution::Half,
                tier: BeamTier::Low
            })
        );
        assert!(down.windows(2).all(|pair| pair[1] < pair[0]));
        let mut up = down.last().copied().unwrap();
        for expected in down.iter().rev().skip(1) {
            up = up.better().unwrap();
            assert_eq!(up, *expected);
        }
        assert_eq!(up.better(), None);
    }

    #[test]
    fn frames_inside_the_budget_change_nothing() {
        let mut governor = Governor::new(16.6, TOP);
        assert_eq!(run(&mut governor, 14.0, 2000), TOP);
    }

    #[test]
    fn frames_over_the_budget_drop_a_step_at_a_time() {
        let mut governor = Governor::new(16.6, TOP);
        let after = run(&mut governor, 30.0, 25);
        assert_eq!(after.tier, BeamTier::High);
        let bottom = run(&mut governor, 30.0, 2000);
        assert_eq!(
            bottom,
            Quality {
                resolution: AirResolution::Half,
                tier: BeamTier::Low
            }
        );
    }

    #[test]
    fn it_climbs_back_slowly_and_no_higher_than_its_ceiling() {
        let ceiling = Quality {
            resolution: AirResolution::Full,
            tier: BeamTier::High,
        };
        let mut governor = Governor::new(16.6, ceiling);
        run(&mut governor, 40.0, 400);
        assert!(governor.quality() < ceiling);
        let soon = run(&mut governor, 5.0, 100);
        assert!(soon < ceiling, "climbed within a hundred frames");
        assert_eq!(run(&mut governor, 5.0, 5000), ceiling);
    }

    #[test]
    fn a_single_slow_frame_does_not_move_it() {
        let mut governor = Governor::new(16.6, TOP);
        run(&mut governor, 10.0, 500);
        governor.observe(200.0);
        assert_eq!(run(&mut governor, 10.0, 100), TOP);
    }

    #[test]
    fn it_does_not_hunt_at_the_edge_of_the_budget() {
        // A frame time that is over budget at one step and comfortably
        // under at the next would bounce without the headroom to climb.
        let mut governor = Governor::new(16.6, TOP);
        let mut changes = 0_u16;
        let mut last = governor.quality();
        for _ in 0..6000_u16 {
            let frame_ms = if governor.quality() == TOP { 19.0 } else { 13.0 };
            let now = governor.observe(frame_ms);
            if now != last {
                changes += 1;
                last = now;
            }
        }
        assert_eq!(changes, 1, "it dropped once and stayed");
    }

    #[test]
    fn tiers_are_named_and_found_by_name() {
        for tier in BeamTier::ALL {
            assert_eq!(BeamTier::named(tier.name()), Some(tier));
        }
        assert_eq!(BeamTier::named("froxels"), None);
    }
}
