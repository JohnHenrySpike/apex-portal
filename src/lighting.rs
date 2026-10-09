//! RGB lighting stored on the pad: a 380-byte blob (20-byte header, then
//! 10 frames x 12 LEDs x RGB). The pad only plays the stored frames, so every
//! effect is generated here as frame data.

use anyhow::Result;

use crate::hid::{Device, blob};

pub const CMD_READ: u8 = 167;
pub const CMD_WRITE_START: u8 = 168;
pub const CMD_WRITE_PACK: u8 = 169;

const OFF_CLICK_FEEDBACK: usize = 2; // latches the displayed frame on a k5; we keep it 0
const OFF_LOOP_START: usize = 3;
const OFF_LOOP_END: usize = 4;
const OFF_CYCLE: usize = 5;
const OFF_BRIGHTNESS: usize = 6;
const OFF_LED_COUNT: usize = 7;
const OFF_MODE: usize = 8;
const OFF_FRAMES: usize = 20;
// FLOW is 1 both in Space Station's `LedType` and in the ids older builds of
// this app stored, so it is the one mode byte value that can be trusted.
const LED_TYPE_FLOW: u8 = 1;
// Byte 9 (grip sync, "lights react to vibration") is never written by this app.

pub type Rgb = [u8; 3];

/// Where each of the 12 LEDs sits across the pad, left to right (0..1), so
/// effects follow the pad rather than the slot order. Slots 1 and 0 light the
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

/// The LED's place for effects, spread over 0..(n-1)/n like evenly spaced
/// slots, since colour lists wrap around at 1.
fn position(slot: usize, leds: usize) -> f32 {
    let along = if leds == LED_X.len() {
        LED_X[slot] * (leds - 1) as f32
    } else {
        slot as f32
    };
    along / leds as f32
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
    /// Space Station's `LedType` for the mode byte (FLOW 1, BREATH 2,
    /// GRADIENT 3, ON 5, Close 6), so its UI names the stored effect. Several
    /// of our effects share GRADIENT; `LedConfig::effect` tells them apart.
    fn led_type(self) -> u8 {
        match self {
            Effect::Flow => LED_TYPE_FLOW,
            Effect::Breath => 2,
            Effect::Solid | Effect::StaticMulti => 5,
            Effect::Off => 6,
            Effect::Rotation | Effect::Wave | Effect::Rainbow | Effect::Flash => 3,
        }
    }
    pub fn uses_colours(self) -> bool {
        !matches!(self, Effect::Off | Effect::Rainbow)
    }
    pub fn animated(self) -> bool {
        !matches!(self, Effect::Off | Effect::Solid | Effect::StaticMulti)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LedConfig {
    pub blob: Vec<u8>,
}

fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8)
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

impl LedConfig {
    pub fn read(dev: &mut Device) -> Result<Self> {
        Ok(LedConfig {
            blob: blob::read(dev, CMD_READ, 0, "lighting config")?,
        })
    }

    pub fn write(&self, dev: &mut Device, old: Option<&LedConfig>) -> Result<usize> {
        blob::write(
            dev,
            CMD_WRITE_START,
            CMD_WRITE_PACK,
            0,
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
    pub fn mode(&self) -> u8 {
        self.blob[OFF_MODE]
    }
    /// Which of our effects made the stored frames. The mode byte holds only
    /// the coarser `LedType` (older builds stored their own ids there), so the
    /// frames decide and the byte only separates Flow from look-alikes.
    pub fn effect(&self) -> Effect {
        let (leds, frames) = (self.leds(), self.frames());
        let frame = |f: usize| -> Vec<Rgb> { (0..leds).map(|l| self.led(f, l)).collect() };
        let dark = |f: usize| frame(f).iter().all(|c| *c == [0; 3]);
        let uniform = |f: usize| frame(f).windows(2).all(|w| w[0] == w[1]);
        let flow = self.mode() == LED_TYPE_FLOW;
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
        if frame(0).iter().filter(|c| **c == [0; 3]).count() * 2 > leds {
            return Effect::Rotation;
        }
        if flow { Effect::Flow } else { Effect::Wave }
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
                let c = f(fr as f32 / frames as f32, position(l, leds));
                let o = OFF_FRAMES + (fr * leds + l) * 3;
                self.blob[o..o + 3].copy_from_slice(&c);
            }
        }
        self.blob[OFF_LOOP_START] = 0;
        self.blob[OFF_LOOP_END] = (frames - 1) as u8;
        self.blob[OFF_MODE] = effect.led_type();
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
            Effect::Rotation => self.fill(effect, |f, l| {
                let span = (leds / 3.0).max(1.0);
                let behind = (f * leds - l * leds).rem_euclid(leds);
                let c = cols[(f * 10.0) as usize % n];
                scale(c, (1.0 - behind / span).max(0.0))
            }),
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
        LedConfig { blob }
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
    fn mode_byte_is_space_stations_led_type() {
        for (effect, ty) in [
            (Effect::Flow, 1),
            (Effect::Breath, 2),
            (Effect::Wave, 3),
            (Effect::Rainbow, 3),
            (Effect::Solid, 5),
            (Effect::StaticMulti, 5),
            (Effect::Off, 6),
        ] {
            let mut cfg = blank();
            cfg.apply(effect, &[[255, 0, 0], [0, 0, 255]]);
            assert_eq!(cfg.mode(), ty, "{effect:?}");
        }
    }

    #[test]
    fn factory_lighting_reads_as_an_animation() {
        // the factory rainbow is stored as Default (7), not one of ours
        let cfg = LedConfig {
            blob: include_bytes!("../led_backup_original.bin").to_vec(),
        };
        assert!(cfg.effect().animated());
    }
}
