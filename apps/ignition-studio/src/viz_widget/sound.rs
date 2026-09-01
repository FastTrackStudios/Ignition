//! The sound fade: a meter smoothed into something a light can follow.
//!
//! Pure arithmetic, and the only part of the visualizer widget that is.
//! An input meter jumps per audio block; a fixture driven straight off
//! one strobes. [`SoundFade::step`] is an exponential approach, so a
//! kick reads as a lift with a tail — which is what "fade" means at a
//! desk — and it is a function of `(state, dt)` with no clock, no
//! device and no Bevy in it, which is why it can be tested by handing
//! it timesteps.

use crate::viz_widget::EmbeddedViz;
use ignition_viz::playback::Playback;

#[derive(Debug, Clone, PartialEq)]
/// The sound fade: a one-pole smoother over the band levels, with the
/// time constant the operator sets.
///
/// Here rather than in the engine because `Show::sound` is defined as
/// *already smoothed* — a recipe stays a pure function of what it is
/// handed, and the same recipe on the same levels is the same value
/// everywhere. The host owns the time.
// r[impl playback.sound-as-value] - the sound fade, smoothing the levels the recipes read
pub struct SoundFade {
    /// Seconds to settle, 0–2. Zero passes the raw meter through.
    pub secs: f32,
    /// What the input last reported.
    pub raw: [f32; 3],
    /// What the recipes read.
    pub smoothed: [f32; 3],
    last_step: Option<std::time::Instant>,
}

impl Default for SoundFade {
    fn default() -> Self {
        Self {
            secs: 0.25,
            raw: [0.0; 3],
            smoothed: [0.0; 3],
            last_step: None,
        }
    }
}

impl SoundFade {
    /// The longest fade the slider offers.
    pub const MAX_SECS: f32 = 2.0;

    /// Advances the smoothing by `dt` seconds. A fade of zero snaps.
    /// Exponential rather than linear so a kick reads as a lift with a
    /// tail, which is what "fade" means at a desk.
    pub fn step(&mut self, dt: f32) -> [f32; 3] {
        let k = if self.secs <= 0.0 || dt <= 0.0 {
            1.0
        } else {
            (1.0 - (-dt / self.secs).exp()).clamp(0.0, 1.0)
        };
        for (s, r) in self.smoothed.iter_mut().zip(self.raw) {
            *s += k * (r - *s);
        }
        self.smoothed
    }

    /// Steps by the wall clock since the last call.
    fn tick(&mut self) -> [f32; 3] {
        let now = std::time::Instant::now();
        let dt = self
            .last_step
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.0);
        self.last_step = Some(now);
        self.step(dt)
    }
}

/// Writes the smoothed band levels into the engine for this frame.
///
/// Every frame, even when nothing arrived: the fade is what makes a
/// kick decay rather than vanish, and the decay happens between inputs.
// r[impl playback.sound-as-value] - `Show.sound` is written every frame
pub(super) fn smooth_sound(fade: &mut SoundFade, viz: &mut EmbeddedViz) {
    let [low, mid, high] = fade.tick();
    let world = viz.app_mut().world_mut();
    if let Some(mut playback) = world.get_resource_mut::<Playback>() {
        playback.sound = ignition_viz::playback::SoundLevels { low, mid, high };
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::SoundFade;

    /// r[verify playback.sound-as-value]
    #[test]
    fn a_zero_fade_snaps_and_a_long_one_lags() {
        let mut snap = SoundFade {
            secs: 0.0,
            raw: [1.0, 0.5, 0.0],
            ..Default::default()
        };
        assert_eq!(snap.step(0.016), [1.0, 0.5, 0.0]);
        let mut slow = SoundFade {
            secs: 1.0,
            raw: [1.0, 0.0, 0.0],
            ..Default::default()
        };
        let first = slow.step(0.1)[0];
        assert!(first > 0.0 && first < 0.2, "{first}");
        // Approaches the raw level and never overshoots.
        let mut last = first;
        for _ in 0..100 {
            let next = slow.step(0.1)[0];
            assert!(next >= last && next <= 1.0);
            last = next;
        }
        assert!(last > 0.99, "{last}");
        // And decays when the input stops.
        slow.raw = [0.0; 3];
        assert!(slow.step(0.5)[0] < last);
    }
}
