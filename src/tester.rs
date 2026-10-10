//! What the input tester gathers from every report of the vendor stream:
//! the latest state, the polling rate and how far each stick reaches.

use std::time::{Duration, Instant};

use crate::device::InputState;
use crate::evdev::Output;

/// Angular sectors of the stick gate, 5° each.
pub const SECTORS: usize = 72;

/// A deflection below this does not count as reaching the edge.
const EDGE: f32 = 0.5;

/// The farthest each stick got in every direction.
#[derive(Clone, Copy)]
pub struct Gate {
    /// Radius per sector, 0 where the stick has not been; sector 0 is +X,
    /// counter-clockwise with +Y up.
    pub reach: [f32; SECTORS],
}

impl Default for Gate {
    fn default() -> Self {
        Gate {
            reach: [0.0; SECTORS],
        }
    }
}

impl Gate {
    pub fn add(&mut self, x: f32, y: f32) {
        let r = x.hypot(y);
        if r < EDGE {
            return;
        }
        let turn = y.atan2(x).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        let i = ((turn * SECTORS as f32).round() as usize) % SECTORS;
        self.reach[i] = self.reach[i].max(r);
    }

    /// Share of directions the stick has been pushed to the edge in.
    pub fn coverage(&self) -> f32 {
        self.reach.iter().filter(|r| **r > 0.0).count() as f32 / SECTORS as f32
    }

    /// Mean distance from the unit circle in %, once every direction is in.
    pub fn error(&self) -> Option<f32> {
        if self.coverage() < 1.0 {
            return None;
        }
        Some(self.reach.iter().map(|r| (r - 1.0).abs()).sum::<f32>() / SECTORS as f32 * 100.0)
    }
}

/// Stick axis in -1..1.
pub fn axis(v: i16) -> f32 {
    (v as f32 / 32767.0).clamp(-1.0, 1.0)
}

#[derive(Default)]
pub struct Live {
    pub last: Option<(InputState, Instant)>,
    /// Reports per second over the last full second.
    pub rate: Option<f32>,
    /// From the vendor stream: the sensor before the profile.
    pub gates: [Gate; 2],
    /// From the gamepad node: what games get.
    pub out_gates: [Gate; 2],
    /// The gamepad node's latest state.
    pub out: Option<Output>,
    window: Option<(Instant, u32)>,
}

impl Live {
    pub fn record(&mut self, s: InputState) {
        let now = Instant::now();
        self.last = Some((s, now));
        self.gates[0].add(axis(s.lx), axis(s.ly));
        self.gates[1].add(axis(s.rx), axis(s.ry));
        let (start, n) = self.window.get_or_insert((now, 0));
        *n += 1;
        let took = now - *start;
        if took >= Duration::from_secs(1) {
            self.rate = Some(*n as f32 / took.as_secs_f32());
            self.window = Some((now, 0));
        }
    }

    pub fn record_output(&mut self, o: Output) {
        for (g, (x, y)) in self.out_gates.iter_mut().zip(o.sticks) {
            g.add(x, y);
        }
        self.out = Some(o);
    }

    pub fn reset_gates(&mut self) {
        self.gates = Default::default();
        self.out_gates = Default::default();
    }

    /// The stream stopped: forget the state and the rate, keep the gates.
    pub fn stop(&mut self) {
        self.last = None;
        self.rate = None;
        self.window = None;
        self.out = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(g: &mut Gate, r: f32) {
        for i in 0..720 {
            let a = i as f32 / 720.0 * std::f32::consts::TAU;
            g.add(r * a.cos(), r * a.sin());
        }
    }

    #[test]
    fn a_full_circle_covers_the_gate() {
        let mut g = Gate::default();
        assert_eq!(g.error(), None);
        circle(&mut g, 1.0);
        assert_eq!(g.coverage(), 1.0);
        assert!(g.error().unwrap() < 0.01);
    }

    #[test]
    fn small_moves_do_not_count() {
        let mut g = Gate::default();
        circle(&mut g, 0.3);
        assert_eq!(g.coverage(), 0.0);
    }

    #[test]
    fn a_square_gate_reads_as_off_round() {
        let mut g = Gate::default();
        for i in 0..720 {
            let a = i as f32 / 720.0 * std::f32::consts::TAU;
            let (x, y) = (a.cos(), a.sin());
            let k = x.abs().max(y.abs());
            g.add(x / k, y / k);
        }
        // a square reaches √2 in the corners and 1 on the sides
        let e = g.error().unwrap();
        assert!((10.0..20.0).contains(&e), "{e}");
    }

    #[test]
    fn a_half_circle_has_no_error_yet() {
        let mut g = Gate::default();
        for i in 0..360 {
            let a = i as f32 / 720.0 * std::f32::consts::TAU;
            g.add(a.cos(), a.sin());
        }
        assert!((g.coverage() - 0.5).abs() < 0.03);
        assert_eq!(g.error(), None);
    }
}
