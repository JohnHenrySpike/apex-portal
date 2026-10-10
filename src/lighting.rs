//! RGB lighting stored on the pad: a 380-byte blob (20-byte header, then
//! 10 frames x 12 LEDs x RGB). The pad only plays the stored frames, so every
//! effect is generated here as frame data.

use anyhow::Result;

use crate::hid::{Device, blob};

pub const CMD_READ: u8 = 167;
pub const CMD_WRITE_START: u8 = 168;
pub const CMD_WRITE_PACK: u8 = 169;

// "click feedback": Space Station offers it for the Apex 4 only; on an Apex 5
// the LEDs do not react to presses with it (HW). We keep it 0.
const OFF_CLICK_FEEDBACK: usize = 2;
const OFF_LOOP_START: usize = 3;
const OFF_LOOP_END: usize = 4;
const OFF_CYCLE: usize = 5;
const OFF_BRIGHTNESS: usize = 6;
const OFF_LED_COUNT: usize = 7;
const OFF_MODE: usize = 8;
const OFF_GRIP_SYNC: usize = 9; // "light effect on vibration": 1 = flare with rumble
const OFF_FRAMES: usize = 20;
// Flow is 1 both in this app's ids and in Space Station's `LedType`.
const MODE_FLOW: u8 = 1;

pub type Rgb = [u8; 3];

/// Where each of the 12 LEDs sits across the pad, left to right (0..1), for
/// drawing the pad. Effects run in slot order instead: spreading them by
/// these places made them play unevenly on the pad. Slots 1 and 0 light the
/// FLYDIGI plate on the left grip (left, right), slots 2..11 the strip along
/// the bottom edge, left to right. Taken from the LED marks in
/// assets/apex5.svg; src/ui/pad.rs checks that they still match.
pub const LED_X: [f32; 12] = [
    0.117, 0.0, 0.213, 0.345, 0.419, 0.512, 0.582, 0.631, 0.700, 0.793, 0.867, 1.0,
];

/// Slots ordered as their LEDs sit on the pad, left to right.
pub fn left_to_right(leds: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..leds).collect();
    if leds == LED_X.len() {
        order.sort_by(|&a, &b| LED_X[a].total_cmp(&LED_X[b]));
    }
    order
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Effect {
    Off,
    Solid,
    Breath,
    Flow,
    Rotation,
    Wave,
    Rainbow,
    Flash,
    StaticMulti,
}

impl Effect {
    pub const ALL: [Effect; 9] = [
        Effect::Solid,
        Effect::Breath,
        Effect::Flow,
        Effect::Rotation,
        Effect::Wave,
        Effect::Rainbow,
        Effect::Flash,
        Effect::StaticMulti,
        Effect::Off,
    ];
    /// The mode byte. The firmware reads it while playing the frames (HW:
    /// with Space Station's `LedType` codes, BREATH 2 and GRADIENT 3, Breath
    /// and Rotation played with jerks), so these are the ids every effect was
    /// seen to play smoothly with, even if Space Station names them wrongly.
    fn mode(self) -> u8 {
        match self {
            Effect::Off => 0,
            Effect::Flow => MODE_FLOW,
            Effect::Rotation => 2,
            Effect::Breath => 3,
            Effect::Solid => 4,
            Effect::StaticMulti => 5,
            Effect::Rainbow => 6,
            Effect::Wave => 7,
            Effect::Flash => 8,
        }
    }
    pub fn uses_colours(self) -> bool {
        !matches!(self, Effect::Off | Effect::Rainbow)
    }
    pub fn animated(self) -> bool {
        !matches!(self, Effect::Off | Effect::Solid | Effect::StaticMulti)
    }
}

/// Lighting is kept per profile slot (HW: slot 1 differed from 2-4).
#[derive(Clone, PartialEq, Eq)]
pub struct LedConfig {
    pub slot: u8,
    pub blob: Vec<u8>,
}

fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8)
}

/// Whether one colour is a dimmed copy of the other (`scale` truncates, so
/// each channel may be off by one).
fn same_hue(a: Rgb, b: Rgb) -> bool {
    let (hi, lo) = if a.iter().max() >= b.iter().max() {
        (a, b)
    } else {
        (b, a)
    };
    let top = *hi.iter().max().unwrap() as f32;
    if top == 0.0 {
        return true;
    }
    let k = *lo.iter().max().unwrap() as f32 / top;
    hi.iter()
        .zip(lo)
        .all(|(&h, l)| (h as f32 * k - l as f32).abs() <= 1.5)
}

fn sample(cols: &[Rgb], p: f32) -> Rgb {
    let n = cols.len();
    if n == 1 {
        return cols[0];
    }
    let x = p.rem_euclid(1.0) * n as f32;
    let i = x as usize % n;
    blend(cols[i], cols[(i + 1) % n], x.fract())
}

fn scale(c: Rgb, k: f32) -> Rgb {
    c.map(|v| (v as f32 * k) as u8)
}

fn tri(p: f32) -> f32 {
    1.0 - (2.0 * p.rem_euclid(1.0) - 1.0).abs()
}

pub fn hue(p: f32) -> Rgb {
    let s = p.rem_euclid(1.0) * 6.0;
    let r = (255.0 * s.fract()) as u8;
    let f = 255 - r;
    [
        [255, r, 0],
        [f, 255, 0],
        [0, 255, r],
        [0, f, 255],
        [r, 0, 255],
        [255, 0, f],
    ][s as usize % 6]
}

/// Factory lighting of every Apex 5 slot: a backup read from this pad before
/// any change, byte-equal to the four `ledConfigBean`s of Flydigi's
/// `default_mapping_128.dat` (that file leaves out the grip flash, byte 9).
const FACTORY: &[u8] = include_bytes!("../led_backup_original.bin");

impl LedConfig {
    pub fn factory(slot: u8) -> Self {
        LedConfig {
            slot,
            blob: FACTORY.to_vec(),
        }
    }

    /// Reads the lighting of profile slot `slot`. NOTE: like reading a
    /// profile (163), this makes that slot the running one (HW).
    pub fn read(dev: &mut Device, slot: u8) -> Result<Self> {
        Ok(LedConfig {
            slot,
            blob: blob::read(dev, CMD_READ, slot, "lighting config")?,
        })
    }

    /// Writes to this config's slot, only the packets that differ from `old`
    /// when `old` is the same slot's lighting.
    pub fn write(&self, dev: &mut Device, old: Option<&LedConfig>) -> Result<usize> {
        let old = old.filter(|o| o.slot == self.slot);
        blob::write(
            dev,
            CMD_WRITE_START,
            CMD_WRITE_PACK,
            self.slot,
            &self.blob,
            old.map(|o| &o.blob[..]),
        )
    }

    pub fn leds(&self) -> usize {
        match self.blob[OFF_LED_COUNT] {
            0 => 10,
            n => n as usize,
        }
    }
    pub fn frames(&self) -> usize {
        (self.blob.len().saturating_sub(OFF_FRAMES)) / (3 * self.leds())
    }
    pub fn brightness(&self) -> u8 {
        self.blob[OFF_BRIGHTNESS]
    }
    pub fn set_brightness(&mut self, v: u8) {
        self.blob[OFF_BRIGHTNESS] = v.min(100);
    }
    pub fn cycle(&self) -> u8 {
        self.blob[OFF_CYCLE]
    }
    pub fn set_cycle(&mut self, v: u8) {
        self.blob[OFF_CYCLE] = v.max(1);
    }
    /// Whether the LEDs flare while the grip motors run (byte 9).
    pub fn grip_sync(&self) -> bool {
        self.blob[OFF_GRIP_SYNC] == 1
    }
    pub fn set_grip_sync(&mut self, on: bool) {
        self.blob[OFF_GRIP_SYNC] = on as u8;
    }
    pub fn mode(&self) -> u8 {
        self.blob[OFF_MODE]
    }
    /// Which of our effects made the stored frames. The frames decide, since
    /// the mode byte may hold Space Station's `LedType` codes (its own
    /// lighting, or a build of this app that wrote them); the byte only
    /// separates Flow from look-alikes.
    pub fn effect(&self) -> Effect {
        let (leds, frames) = (self.leds(), self.frames());
        let frame = |f: usize| -> Vec<Rgb> { (0..leds).map(|l| self.led(f, l)).collect() };
        let dark = |f: usize| frame(f).iter().all(|c| *c == [0; 3]);
        let uniform = |f: usize| frame(f).windows(2).all(|w| w[0] == w[1]);
        let flow = self.mode() == MODE_FLOW;
        if (0..frames).all(dark) {
            return Effect::Off;
        }
        if (1..frames).all(|f| frame(f) == frame(0)) {
            return if flow {
                Effect::Flow
            } else if uniform(0) {
                Effect::Solid
            } else {
                Effect::StaticMulti
            };
        }
        let mut rainbow = self.clone();
        rainbow.apply(Effect::Rainbow, &[]);
        if rainbow.blob[OFF_FRAMES..] == self.blob[OFF_FRAMES..] {
            return Effect::Rainbow;
        }
        if (0..frames).all(uniform) {
            return if !dark(0) && dark(1) {
                Effect::Flash
            } else {
                Effect::Breath
            };
        }
        // a wave darkens at most one LED at a time, a comet leaves a gap
        if frame(0).iter().filter(|c| **c == [0; 3]).count() >= 2 {
            return Effect::Rotation;
        }
        if flow { Effect::Flow } else { Effect::Wave }
    }
    /// The colours the frames were most likely made from, at most 5, in
    /// order of first appearance. Dimmed copies of one colour (a fade, a
    /// breath, a rotating tail) fold into its brightest shade, so they do not
    /// come back as colours of their own.
    pub fn colours(&self) -> Vec<Rgb> {
        let sum = |c: Rgb| c.iter().map(|&v| v as u32).sum::<u32>();
        let mut out: Vec<Rgb> = vec![];
        for f in 0..self.frames() {
            for l in 0..self.leds() {
                let c = self.led(f, l);
                if c == [0; 3] {
                    continue;
                }
                match out.iter().position(|o| same_hue(*o, c)) {
                    Some(i) if sum(c) > sum(out[i]) => out[i] = c,
                    Some(_) => {}
                    None if out.len() < 5 => out.push(c),
                    None => {}
                }
            }
        }
        out
    }

    /// Whether applying `effect` with `colours` gives exactly these frames.
    pub fn made_by(&self, effect: Effect, colours: &[Rgb]) -> bool {
        let mut again = self.clone();
        again.apply(effect, colours);
        again.blob[OFF_FRAMES..] == self.blob[OFF_FRAMES..]
    }

    pub fn loop_range(&self) -> (usize, usize) {
        (
            self.blob[OFF_LOOP_START] as usize,
            self.blob[OFF_LOOP_END] as usize,
        )
    }

    pub fn led(&self, frame: usize, led: usize) -> Rgb {
        let o = OFF_FRAMES + (frame * self.leds() + led) * 3;
        [self.blob[o], self.blob[o + 1], self.blob[o + 2]]
    }

    fn fill(&mut self, effect: Effect, f: impl Fn(f32, f32) -> Rgb) {
        let (leds, frames) = (self.leds(), self.frames());
        for fr in 0..frames {
            for l in 0..leds {
                let c = f(fr as f32 / frames as f32, l as f32 / leds as f32);
                let o = OFF_FRAMES + (fr * leds + l) * 3;
                self.blob[o..o + 3].copy_from_slice(&c);
            }
        }
        self.blob[OFF_LOOP_START] = 0;
        self.blob[OFF_LOOP_END] = (frames - 1) as u8;
        self.blob[OFF_MODE] = effect.mode();
        self.blob[OFF_CLICK_FEEDBACK] = 0;
    }

    /// Lights one LED slot and leaves the others dark, to find out which LED
    /// on the pad a slot drives.
    pub fn single(&mut self, slot: usize, colour: Rgb) {
        let leds = self.leds() as f32;
        self.fill(Effect::StaticMulti, |_, l| {
            if (l * leds).round() as usize == slot {
                colour
            } else {
                [0; 3]
            }
        });
    }

    pub fn apply(&mut self, effect: Effect, colours: &[Rgb]) {
        let cols: Vec<Rgb> = if colours.is_empty() {
            vec![[0, 116, 255]]
        } else {
            colours.to_vec()
        };
        let n = cols.len();
        let leds = self.leds() as f32;
        match effect {
            Effect::Off => self.fill(effect, |_, _| [0; 3]),
            Effect::Solid => self.fill(effect, |_, _| cols[0]),
            Effect::StaticMulti => self.fill(effect, |_, l| sample(&cols, l)),
            Effect::Flow => self.fill(effect, |f, l| sample(&cols, l + f)),
            Effect::Rainbow => {
                let wheel: Vec<Rgb> = (0..6).map(|i| hue(i as f32 / 6.0)).collect();
                self.fill(effect, |f, l| sample(&wheel, l + f))
            }
            Effect::Breath => self.fill(effect, |f, _| {
                scale(cols[(f * n as f32) as usize % n], tri(f))
            }),
            Effect::Wave => self.fill(effect, |f, l| scale(sample(&cols, l), tri(l + f))),
            Effect::Rotation => {
                // With 10 frames for 12 LEDs the head moves 1.2 LEDs a frame,
                // so a hard leading edge popped LEDs on. This comet rises over
                // two frames' travel, holds for one and fades over four: every
                // LED reaches full strength and the total light stays constant.
                let step = leds / self.frames() as f32;
                let (rise, hold, fade) = (2.0 * step, step, 4.0 * step);
                self.fill(effect, |f, l| {
                    let mut passed = (f * leds - l * leds).rem_euclid(leds);
                    if passed > leds / 2.0 {
                        passed -= leds; // the head has not reached it yet
                    }
                    let k = if passed < 0.0 {
                        1.0 + passed / rise
                    } else if passed <= hold {
                        1.0
                    } else {
                        1.0 - (passed - hold) / fade
                    };
                    scale(sample(&cols, f), k.max(0.0))
                })
            }
            Effect::Flash => self.fill(effect, |f, _| {
                let fr = (f * 10.0).round() as usize;
                if fr.is_multiple_of(2) {
                    cols[(fr / 2) % n]
                } else {
                    [0; 3]
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> LedConfig {
        let mut blob = vec![0xFF; OFF_FRAMES + 10 * 12 * 3];
        blob[..10].copy_from_slice(&[0, 3, 0, 0, 9, 10, 100, 12, 1, 0]);
        LedConfig { slot: 0, blob }
    }

    #[test]
    fn every_effect_reads_back_from_its_frames() {
        let palettes: [&[Rgb]; 3] = [
            &[[0, 116, 255]],
            &[[255, 0, 0], [0, 255, 0]],
            &[[255, 0, 0], [0, 255, 0], [40, 40, 200]],
        ];
        for effect in Effect::ALL {
            for cols in palettes {
                let mut cfg = blank();
                cfg.apply(effect, cols);
                let want = match effect {
                    // one colour has nothing to flow or spread
                    Effect::StaticMulti if cols.len() == 1 => Effect::Solid,
                    e => e,
                };
                assert_eq!(cfg.effect(), want, "{effect:?} with {} colours", cols.len());
            }
        }
    }

    #[test]
    fn mode_byte_keeps_the_ids_that_play_smoothly() {
        for (effect, ty) in [
            (Effect::Off, 0),
            (Effect::Flow, 1),
            (Effect::Rotation, 2),
            (Effect::Breath, 3),
            (Effect::Solid, 4),
            (Effect::StaticMulti, 5),
            (Effect::Rainbow, 6),
            (Effect::Wave, 7),
            (Effect::Flash, 8),
        ] {
            let mut cfg = blank();
            cfg.apply(effect, &[[255, 0, 0], [0, 0, 255]]);
            assert_eq!(cfg.mode(), ty, "{effect:?}");
        }
    }

    #[test]
    fn faded_copies_are_not_colours_of_their_own() {
        // Several colours blend or never show at full strength, so only a
        // single colour comes back exactly; the app remembers the rest.
        for effect in [
            Effect::Rotation,
            Effect::Breath,
            Effect::Wave,
            Effect::Flash,
        ] {
            let mut cfg = blank();
            cfg.apply(effect, &[[0, 116, 255]]);
            assert_eq!(cfg.colours(), [[0, 116, 255]], "{effect:?}");
            assert!(cfg.made_by(effect, &cfg.colours()), "{effect:?}");
        }
    }

    #[test]
    fn rotation_moves_without_popping() {
        let mut cfg = blank();
        cfg.apply(Effect::Rotation, &[[0, 0, 255]]);
        let (frames, leds) = (cfg.frames(), cfg.leds());
        let blue = |f: usize, l: usize| cfg.led(f, l)[2] as i32;
        for l in 0..leds {
            // every LED reaches full strength
            assert_eq!((0..frames).map(|f| blue(f, l)).max(), Some(255), "LED {l}");
            // and never jumps more than half of it from one frame to the next
            for f in 0..frames {
                let jump = (blue((f + 1) % frames, l) - blue(f, l)).abs();
                assert!(jump <= 128, "LED {l} frame {f}: {jump}");
            }
        }
        // the total light stays put (within rounding, 2 %), so the comet does
        // not pulse as it moves; the hard-edged comet varied by about 30 %
        let totals: Vec<i32> = (0..frames)
            .map(|f| (0..leds).map(|l| blue(f, l)).sum())
            .collect();
        let (lo, hi) = (totals.iter().min().unwrap(), totals.iter().max().unwrap());
        assert!((hi - lo) * 50 <= *hi, "{totals:?}");
    }

    #[test]
    fn factory_lighting_reads_as_an_animation() {
        // the factory rainbow is stored as Default (7), not one of ours
        let mut cfg = LedConfig::factory(0);
        assert!(cfg.effect().animated());
        assert_eq!(cfg.blob.len(), 380);
        assert!(cfg.grip_sync(), "this pad came with the grip flash on");
        let effect = cfg.effect();
        cfg.set_grip_sync(false);
        assert!(!cfg.grip_sync());
        assert_eq!(cfg.effect(), effect, "the flash is not part of the effect");
    }
}
