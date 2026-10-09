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
// Byte 9 (grip sync, "lights react to vibration") is never written by this app.

pub type Rgb = [u8; 3];

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
    /// Space Station's effect id, stored in the mode byte for coherence.
    fn id(self) -> u8 {
        match self {
            Effect::Off => 0,
            Effect::Flow => 1,
            Effect::Rotation => 2,
            Effect::Breath => 3,
            Effect::Solid => 4,
            Effect::StaticMulti => 5,
            Effect::Rainbow => 6,
            Effect::Wave => 7,
            Effect::Flash => 8,
        }
    }
    pub fn from_id(id: u8) -> Option<Effect> {
        Self::ALL.into_iter().find(|e| e.id() == id)
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
        self.blob[OFF_MODE] = effect.id();
        self.blob[OFF_CLICK_FEEDBACK] = 0;
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
