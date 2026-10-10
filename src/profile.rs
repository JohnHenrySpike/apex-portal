//! The 840-byte mapping profile (protocol v3.1) and its commands 161-166.
//! Every byte this module does not interpret is carried through unchanged.

use anyhow::{Result, bail};
use std::time::Duration;

use crate::hid::{Device, blob};

pub const SIZE: usize = 840;
pub const SLOTS: u8 = 4;

const CMD_STATUS: u8 = 161;
const CMD_APPLY: u8 = 162;
const CMD_READ: u8 = 163;
const CMD_WRITE_START: u8 = 164;
const CMD_WRITE_PACK: u8 = 165;
const CMD_SAVE: u8 = 166;

const KEYS: usize = 13;
const STICK_CORE: [usize; 2] = [109, 116];
const TRIGGER_TRAVEL: [usize; 2] = [123, 130];
const GYRO: usize = 137;
const GRIP_VIB: usize = 145; // ..154; only the on/off switches change
/// Grip-rumble switches: master, left, right. 0 = on, 0xFF = off. Space
/// Station flips left and right; the spec calls 145 the master.
const GRIP_SWITCHES: [usize; 3] = [145, 146, 150];
const FORCE: [usize; 2] = [185, 205];
const DATA_VERSION: usize = 225;
const MACRO_PAGE: usize = 230;
const MACRO_PAGE_END: usize = 768;
const TITLE: usize = 770;
const STICK_EXTRA: [usize; 2] = [790, 802];
const MACRO_INTERVAL: usize = 820;

pub const TARGET_MACRO: u8 = 32;
pub const TARGET_KEYBOARD: u8 = 254;
pub const TARGET_IDENTITY: u8 = 255;

/// (ControllerKey id, label) of the keys an Apex 5 has, in UI order.
pub const PAD_KEYS: [(u8, &str); 23] = [
    (4, "A"),
    (5, "B"),
    (7, "X"),
    (8, "Y"),
    (0, "↑"),
    (2, "↓"),
    (3, "←"),
    (1, "→"),
    (10, "LB"),
    (11, "RB"),
    (12, "LT"),
    (13, "RT"),
    (14, "L3"),
    (15, "R3"),
    (6, "View"),
    (9, "Menu"),
    (27, "Home"),
    (18, "M1"),
    (19, "M2"),
    (20, "M3"),
    (21, "M4"),
    (22, "LM"),
    (23, "RM"),
];
/// Keys that exist on the XInput side, i.e. valid remap targets and macro steps.
pub const XINPUT_TARGETS: [u8; 17] = [4, 5, 7, 8, 0, 2, 3, 1, 10, 11, 12, 13, 14, 15, 6, 9, 27];

pub fn key_name(id: u8) -> String {
    PAD_KEYS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("#{id}"))
}

// ------------------------------------------------------------------ model

#[derive(Clone, PartialEq, Eq)]
pub struct Profile {
    pub slot: u8,
    pub blob: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyBinding {
    pub target: u8,
    pub turbo_mode: u8, // 0 off, 1 while held, 2 toggle
    pub turbo_freq: u8, // 0 = turbo off
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stick {
    pub kind: u8, // 0 default, 1 quick, 2 slow, 3 custom
    /// -100..100 %: > 0 dead zone, < 0 offset (output starts there); 127 =
    /// the "not a stick" sentinel, compiled as 0.
    pub center: i8,
    pub p1: (u8, u8),
    pub p2: (u8, u8),
    /// -100..100 %: > 0 outer zone (full output early), < 0 output ceiling.
    pub edge: i8,
    pub round: bool,
    pub bank: [u8; 9],
}

pub const STICK_PRESETS: [(u8, &str, (u8, u8), (u8, u8)); 3] = [
    (0, "Стандарт", (63, 63), (127, 127)),
    (1, "Быстрый", (64, 96), (127, 127)),
    (2, "Плавный", (64, 32), (127, 127)),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Gyro {
    pub kind: u8, // 0 off, 1 left stick, 2 right stick
    pub key: u8,
    pub hold: bool, // false = click toggles
    pub deadzone: u8,
    pub sens_x: u8,
    pub sens_y: u8,
    pub key2: u8,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ForceEffect {
    pub mode: u8,
    pub params: [u8; 10],
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Macro {
    pub key: u8,
    pub kind: u8, // 1 once, 2 while held, 3 toggle
    pub interval_ms: Option<u16>,
    pub steps: Vec<MacroStep>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MacroStep {
    pub delay_ms: u32,
    pub key: u8,
    pub press: bool,
}

pub const MACRO_MAX: usize = 5;
pub const MACRO_STEPS_MAX: usize = 128;

/// Grip-rumble state. The app only flips its on/off switches.
pub struct GripVibration {
    pub master: bool,
    pub left: bool,
    pub right: bool,
}

fn stick_nodes(center: f64, edge: f64, p1: (u8, u8), p2: (u8, u8)) -> Vec<(f64, f64)> {
    let start = if center > 0.0 {
        (center, 0.0)
    } else {
        (0.0, -center)
    };
    let end = if edge > 0.0 {
        (100.0 - edge, 100.0)
    } else {
        (100.0, 100.0 + edge)
    };
    let span = end.0 - start.0;
    if span <= 0.0 {
        return vec![start, end];
    }
    // Interior points sit in the box between start and end, as in Space
    // Station's renderer; with zones >= 0 that box is (start.x, 0)..(end.x, 100).
    let rise = end.1 - start.1;
    let interior = |p: (u8, u8)| {
        (
            start.0 + span * p.0 as f64 / 127.0,
            start.1 + rise * p.1 as f64 / 127.0,
        )
    };
    vec![start, interior(p1), interior(p2), end]
}

fn along(nodes: &[(f64, f64)], x: f64) -> f64 {
    for i in 0..nodes.len() - 1 {
        let ((x0, y0), (x1, y1)) = (nodes[i], nodes[i + 1]);
        if x <= x1 || i == nodes.len() - 2 {
            if x0 == x1 {
                return if x < x0 { y0 } else { y1 };
            }
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    nodes.last().unwrap().1
}

impl Stick {
    /// Clamp the zones to -100..100 and keep the curve rising: a dead zone and
    /// an outer zone, or an offset and a ceiling, share 100 %; the edge gives way.
    pub fn clamp_zones(&mut self) {
        self.center = self.center.clamp(-100, 100);
        self.edge = self.edge.clamp(-100, 100);
        if self.center >= 0 && self.edge > 0 {
            self.edge = self.edge.min(100 - self.center);
        }
        if self.center < 0 && self.edge < 0 {
            self.edge = self.edge.max(-100 - self.center);
        }
    }
    /// The polyline the bank is compiled from: start, p1, p2, end, in %.
    pub fn nodes(&self) -> Vec<(f64, f64)> {
        let zone = |v: i8| if v > 100 { 0.0 } else { v as f64 };
        stick_nodes(zone(self.center), zone(self.edge), self.p1, self.p2)
    }
    /// The control point (0..127) that puts an interior node at (x, y) %.
    pub fn point_at(&self, x: f64, y: f64) -> (u8, u8) {
        let n = self.nodes();
        let (start, end) = (n[0], n[n.len() - 1]);
        let along = |v: f64, a: f64, b: f64| {
            if b > a {
                ((v - a) / (b - a) * 127.0).clamp(0.0, 127.0) as u8
            } else {
                0
            }
        };
        (along(x, start.0, end.0), along(y, start.1, end.1))
    }
    /// Recompile the 9-point output bank the firmware actually plays.
    pub fn compile(&mut self) {
        let nodes = self.nodes();
        for i in 0..9 {
            let y = along(&nodes, 100.0 * i as f64 / 8.0).clamp(-50.0, 100.0);
            self.bank[i] = (y.trunc() + 50.0) as u8;
        }
    }
    /// Output % for an input %, as the firmware plays it from the bank.
    pub fn output(&self, x: f64) -> f64 {
        let pos = (x / 12.5).clamp(0.0, 8.0);
        let i = (pos as usize).min(7);
        let t = pos - i as f64;
        let y0 = self.bank[i] as f64 - 50.0;
        let y1 = self.bank[i + 1] as f64 - 50.0;
        (y0 + (y1 - y0) * t).max(0.0)
    }
}

impl Profile {
    pub fn proto_version(&self) -> u16 {
        u16::from_le_bytes([self.blob[0], self.blob[1]])
    }
    pub fn data_version(&self) -> u16 {
        u16::from_le_bytes([self.blob[DATA_VERSION], self.blob[DATA_VERSION + 1]])
    }

    // ---- title
    pub fn title(&self) -> String {
        let units: Vec<u16> = self.blob[TITLE..TITLE + 20]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0 && *u != 0xFFFF)
            .collect();
        String::from_utf16_lossy(&units)
    }
    pub fn set_title(&mut self, t: &str) {
        if t == self.title() {
            return; // keep the factory 0xFF tail byte-exact
        }
        let units: Vec<u16> = t.encode_utf16().take(10).collect();
        let mut units = units;
        // never end on a lone high surrogate
        if units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
            units.pop();
        }
        let region = &mut self.blob[TITLE..TITLE + 20];
        region.fill(0);
        for (i, u) in units.iter().enumerate() {
            region[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
    }

    // ---- keys
    pub fn key(&self, id: u8) -> KeyBinding {
        let o = KEYS + 3 * id as usize;
        KeyBinding {
            target: self.blob[o],
            turbo_mode: self.blob[o + 1],
            turbo_freq: self.blob[o + 2],
        }
    }
    pub fn set_key(&mut self, id: u8, mut b: KeyBinding) {
        if b.target == id {
            b.target = TARGET_IDENTITY;
        }
        if b.turbo_freq > 0 {
            if b.target >= TARGET_KEYBOARD {
                b.target = id;
            }
        } else {
            b.turbo_mode = 0;
        }
        let o = KEYS + 3 * id as usize;
        self.blob[o..o + 3].copy_from_slice(&[b.target, b.turbo_mode, b.turbo_freq]);
    }

    // ---- sticks
    pub fn stick(&self, side: usize) -> Stick {
        let c = STICK_CORE[side];
        let x = STICK_EXTRA[side];
        let b = &self.blob;
        let bank: [u8; 9] = b[x + 1..x + 10].try_into().unwrap();
        // Negative zones live only in the bank (the core bytes stay 0, see
        // set_stick): an offset lifts bank[0] above 50, a ceiling drops
        // bank[8] below 150. Space Station's own two's-complement core
        // bytes (> 100) are read the same way.
        let mut center = match b[c + 1] {
            v @ 0..=100 => v as i8,
            127 => 127,
            _ => 0,
        };
        if center <= 0 && bank[0] > 50 {
            center = -((bank[0] - 50).min(100) as i8);
        }
        let mut edge = if b[x + 11] <= 100 { b[x + 11] as i8 } else { 0 };
        if edge <= 0 && bank[8] < 150 {
            edge = -((150 - bank[8].max(50)) as i8);
        }
        Stick {
            kind: b[c],
            center,
            p1: (b[c + 2], b[c + 3]),
            p2: (b[c + 4], b[c + 5]),
            edge,
            round: b[x + 10] == 1,
            bank,
        }
    }
    pub fn set_stick(&mut self, side: usize, mut s: Stick) {
        s.clamp_zones();
        s.p1 = (s.p1.0.min(127), s.p1.1.min(127));
        s.p2 = (s.p2.0.min(127), s.p2.1.min(127));
        s.compile();
        let c = STICK_CORE[side];
        let x = STICK_EXTRA[side];
        let b = &mut self.blob;
        // Negative zones go only into the bank: Space Station writes them as
        // two's complement but reads them back as 127 - byte.
        let center = s.center.max(0) as u8;
        b[c..c + 6].copy_from_slice(&[s.kind, center, s.p1.0, s.p1.1, s.p2.0, s.p2.1]);
        b[x] = s.kind;
        b[x + 1..x + 10].copy_from_slice(&s.bank);
        b[x + 10] = s.round as u8;
        b[x + 11] = s.edge.max(0) as u8;
    }

    // ---- trigger travel window (0..255)
    pub fn travel(&self, side: usize) -> (u8, u8) {
        let o = TRIGGER_TRAVEL[side];
        (self.blob[o + 1], self.blob[o + 6])
    }
    pub fn set_travel(&mut self, side: usize, zero: u8, end: u8) {
        let (z, e) = (zero.min(end), zero.max(end));
        let o = TRIGGER_TRAVEL[side];
        self.blob[o + 1..o + 7].copy_from_slice(&[z, z, z, e, e, e]);
    }

    // ---- gyro to stick
    pub fn gyro(&self) -> Gyro {
        let b = &self.blob[GYRO..GYRO + 8];
        Gyro {
            kind: b[0].min(2),
            key: b[1],
            hold: b[2] == 1,
            deadzone: b[3],
            sens_x: b[4],
            sens_y: b[5],
            key2: b[7],
        }
    }
    pub fn set_gyro(&mut self, g: Gyro) {
        let b = &mut self.blob[GYRO..GYRO + 8];
        b[0] = g.kind.min(2);
        b[1] = g.key;
        b[2] = g.hold as u8;
        b[3] = g.deadzone.min(100);
        b[4] = g.sens_x.min(100);
        b[5] = g.sens_y.min(100);
        match g.kind {
            1 => b[6] = 1, // left stick -> racer
            2 => b[6] = 0, // right stick -> FPS
            _ => {}
        }
        if g.hold {
            b[7] = g.key2;
        }
    }

    // ---- grip vibration (read only)
    pub fn grip_vibration(&self) -> GripVibration {
        let b = &self.blob[GRIP_VIB..];
        GripVibration {
            master: b[0] == 0,
            left: b[1] == 0,
            right: b[5] == 0,
        }
    }

    // ---- stored adaptive trigger effect
    pub fn force(&self, side: usize) -> ForceEffect {
        let o = FORCE[side];
        ForceEffect {
            mode: self.blob[o],
            params: self.blob[o + 10..o + 20].try_into().unwrap(),
        }
    }
    /// Only resistance modes (0,1,3,4); the bind half is never touched.
    pub fn set_force(&mut self, side: usize, f: ForceEffect) {
        if !crate::safety::RESISTANCE_MODES.contains(&f.mode) {
            return;
        }
        let o = FORCE[side];
        self.blob[o] = f.mode;
        if f.mode != 0 {
            self.blob[o + 10..o + 20].copy_from_slice(&f.params);
        }
    }

    // ---- macros (v3.1 page inside the profile)
    pub fn macros(&self) -> Vec<Macro> {
        let b = &self.blob;
        let count = b[MACRO_PAGE] as usize;
        if !(1..=MACRO_MAX).contains(&count) {
            return vec![];
        }
        let base = MACRO_PAGE + 6;
        let mut out = vec![];
        for s in 0..count {
            let start = base + 4 * b[MACRO_PAGE + 1 + s] as usize;
            let end = if s + 1 < count {
                base + 4 * b[MACRO_PAGE + 2 + s] as usize
            } else {
                MACRO_PAGE_END
            };
            if start + 4 > end || end > MACRO_PAGE_END {
                continue;
            }
            let n = (u16::from_le_bytes([b[start + 1], b[start + 2]]) as usize)
                .min((end - start - 4) / 4);
            let mut prev = 0u32;
            let steps = (0..n)
                .map(|k| {
                    let o = start + 4 + 4 * k;
                    let cum = u16::from_le_bytes([b[o], b[o + 1]]) as u32;
                    let st = MacroStep {
                        delay_ms: cum.saturating_sub(prev) * 10,
                        key: b[o + 2],
                        press: b[o + 3] == 1,
                    };
                    prev = cum;
                    st
                })
                .collect();
            let iv = b[MACRO_INTERVAL + s];
            out.push(Macro {
                key: b[start],
                kind: b[start + 3],
                interval_ms: (iv != 0xFF).then_some(iv as u16 * 10),
                steps,
            });
        }
        out
    }

    pub fn set_macros(&mut self, macros: &[Macro]) -> Result<()> {
        if macros.len() > MACRO_MAX {
            bail!("не больше {MACRO_MAX} макросов");
        }
        let total: usize = macros.iter().map(|m| m.steps.len()).sum();
        if total > MACRO_STEPS_MAX {
            bail!("в макросах всего не больше {MACRO_STEPS_MAX} шагов");
        }
        let mut keys = std::collections::HashSet::new();
        for m in macros {
            if !keys.insert(m.key) {
                bail!("на кнопке {} уже есть макрос", key_name(m.key));
            }
            if m.steps.iter().any(|s| !XINPUT_TARGETS.contains(&s.key)) {
                bail!("в шаге макроса можно использовать только обычные кнопки Xbox");
            }
        }
        // Keys that had a macro and no longer do go back to identity; keys
        // with a macro point at it. Orphaned bodies would keep playing.
        for old in self.macros() {
            if !keys.contains(&old.key) && self.key(old.key).target == TARGET_MACRO {
                self.set_key(
                    old.key,
                    KeyBinding {
                        target: TARGET_IDENTITY,
                        turbo_mode: 0,
                        turbo_freq: 0,
                    },
                );
            }
        }
        let page = &mut self.blob[MACRO_PAGE..MACRO_PAGE_END];
        page.fill(0xFF);
        page[0..6].fill(0);
        page[0] = macros.len() as u8;
        let mut word = 0usize;
        for (s, m) in macros.iter().enumerate() {
            page[1 + s] = word as u8;
            let o = 6 + 4 * word;
            page[o] = m.key;
            page[o + 1..o + 3].copy_from_slice(&(m.steps.len() as u16).to_le_bytes());
            page[o + 3] = m.kind.clamp(1, 3);
            let mut ticks: u32 = 0;
            for (k, st) in m.steps.iter().enumerate() {
                ticks = (ticks + st.delay_ms / 10).min(0xFFFF);
                let so = o + 4 + 4 * k;
                page[so..so + 2].copy_from_slice(&(ticks as u16).to_le_bytes());
                page[so + 2] = st.key;
                page[so + 3] = st.press as u8;
            }
            word += 1 + m.steps.len();
        }
        for s in 0..MACRO_MAX {
            self.blob[MACRO_INTERVAL + s] = match macros.get(s) {
                Some(m) => match m.interval_ms {
                    Some(ms) => (ms.min(2540) / 10) as u8,
                    None => 3,
                },
                None => 0xFF,
            };
        }
        for m in macros {
            self.set_key(
                m.key,
                KeyBinding {
                    target: TARGET_MACRO,
                    turbo_mode: 0,
                    turbo_freq: 0,
                },
            );
        }
        Ok(())
    }

    pub fn macro_bytes_differ(&self, other: &Profile) -> bool {
        self.blob[MACRO_PAGE..MACRO_PAGE_END] != other.blob[MACRO_PAGE..MACRO_PAGE_END]
            || self.blob[MACRO_INTERVAL..MACRO_INTERVAL + 5]
                != other.blob[MACRO_INTERVAL..MACRO_INTERVAL + 5]
    }

    /// Switch grip rumble on or off: master, left and right together (0 = on,
    /// 0xFF = off). The motors' strength windows stay as they are.
    pub fn set_grip_vibration(&mut self, on: bool) {
        for o in GRIP_SWITCHES {
            self.blob[o] = if on { 0 } else { 0xFF };
        }
    }

    /// Refuse any write that would change rumble state: grip vibration,
    /// trigger motors, the rumble-to-trigger bind, or a vibrating effect mode.
    /// The one change let through is a grip on/off switch set to 0 or 0xFF.
    pub fn check_no_vibration_change(&self, original: &Profile) -> Result<()> {
        let switched = |o: usize| GRIP_SWITCHES.contains(&o) && matches!(self.blob[o], 0 | 0xFF);
        if (GRIP_VIB..183).any(|o| self.blob[o] != original.blob[o] && !switched(o)) {
            bail!("запись отменена: изменились бы настройки вибрации");
        }
        for o in FORCE {
            if self.blob[o + 1..o + 10] != original.blob[o + 1..o + 10] {
                bail!("запись отменена: изменилась бы привязка вибрации к куркам");
            }
            let mode = self.blob[o];
            if mode != original.blob[o] && !crate::safety::RESISTANCE_MODES.contains(&mode) {
                bail!("запись отменена: вибрационный режим курка");
            }
        }
        Ok(())
    }

    /// This profile's settings for `target`'s slot. What this app never
    /// writes stays as `target` has it: grip vibration strengths (the on/off
    /// switches do come along), the triggers' vibration binding, a trigger
    /// mode that is not a resistance, and the data version.
    pub fn onto(&self, target: &Profile) -> Profile {
        let mut b = self.blob.clone();
        let t = &target.blob;
        for o in GRIP_VIB..183 {
            if !(GRIP_SWITCHES.contains(&o) && matches!(b[o], 0 | 0xFF)) {
                b[o] = t[o];
            }
        }
        for o in FORCE {
            b[o + 1..o + 10].copy_from_slice(&t[o + 1..o + 10]);
            if !crate::safety::RESISTANCE_MODES.contains(&b[o]) {
                b[o] = t[o];
            }
        }
        b[DATA_VERSION..DATA_VERSION + 2].copy_from_slice(&t[DATA_VERSION..DATA_VERSION + 2]);
        Profile {
            slot: target.slot,
            blob: b,
        }
    }

    /// Factory content for this slot, keeping this profile's vibration bytes.
    pub fn factory_reset(&mut self) {
        let mut f = factory_blob(self.slot);
        f[GRIP_VIB..225].copy_from_slice(&self.blob[GRIP_VIB..225]);
        f[DATA_VERSION..DATA_VERSION + 2]
            .copy_from_slice(&self.blob[DATA_VERSION..DATA_VERSION + 2]);
        self.blob = f;
    }
}

// ------------------------------------------------------------ transport

pub struct Status {
    pub active: u8,
    pub versions: [u16; 4],
}

pub fn status(dev: &mut Device) -> Result<Status> {
    let Some(r) = dev.query(CMD_STATUS, &[], Duration::from_secs(1))? else {
        bail!("нет ответа на запрос профиля (геймпад спит? нажмите кнопку)")
    };
    let raw = r.at(6);
    let active = match raw {
        0..=3 => raw,
        4..=7 => raw - 4,
        _ => 0,
    };
    let versions = std::array::from_fn(|i| u16::from_le_bytes([r.at(7 + 2 * i), r.at(8 + 2 * i)]));
    Ok(Status { active, versions })
}

/// Reads slot `slot`. NOTE: this makes it the running profile on the pad.
pub fn read(dev: &mut Device, slot: u8) -> Result<Profile> {
    let blob = blob::read(dev, CMD_READ, slot, "profile")?;
    if blob.len() != SIZE {
        bail!("unexpected profile size {}", blob.len());
    }
    Ok(Profile { slot, blob })
}

/// Write changed packets to the running slot; re-apply if macros changed.
pub fn write(dev: &mut Device, new: &Profile, old: &Profile) -> Result<usize> {
    new.check_no_vibration_change(old)?;
    let n = blob::write(
        dev,
        CMD_WRITE_START,
        CMD_WRITE_PACK,
        new.slot,
        &new.blob,
        Some(&old.blob),
    )?;
    if new.macro_bytes_differ(old) && !dev.acked(CMD_APPLY, &[new.slot])? {
        bail!("геймпад не подтвердил применение профиля");
    }
    Ok(n)
}

/// Commit the running profile to flash. `tag` = new data_version.
pub fn save(dev: &mut Device, slot: u8, tag: u16) -> Result<()> {
    let st = status(dev)?;
    if st.active != slot {
        bail!(
            "на геймпаде сейчас активен профиль {}, а не {}",
            st.active + 1,
            slot + 1
        );
    }
    if dev
        .query(CMD_SAVE, &tag.to_le_bytes(), Duration::from_secs(10))?
        .is_none()
    {
        bail!("геймпад не подтвердил сохранение");
    }
    Ok(())
}

/// A fresh change tag: never 0xFFFF, never equal to the current one.
pub fn next_tag(current: u16) -> u16 {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
        .unwrap_or(1);
    let mut t = (seed.wrapping_mul(2654435761) >> 16) as u16 % 0xFFFF;
    if t == current {
        t = (t + 1) % 0xFFFF;
    }
    t
}

const FACTORY_HEX: &str = concat!(
    "01034d200428a00000ff000000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000",
    "ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff",
    "0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff0000ff000000003f3f7f7f7f00003f3f",
    "7f7f7f00000000ffffff00000000ffffff000c00041914000000003cff320050ff3200011e500501",
    "3200ff287805003200011e5005013200ff287805003200000000000a0a6401ff4600000000000000",
    "000000000000000a0a6401ff46000000000000000000000000ffffffffff000000000000ffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "ffffffffffffffffffff4d916e7f310000000000000000000000ffffffff00323e4b5764707d8996",
    "000000323e4b5764707d89960000ffffffffffff0303030303ffffffffff003f3f7f7f7fffffffff",
);

/// Factory Apex 5 profile for `slot` (titles differ only at byte 774).
fn factory_blob(slot: u8) -> Vec<u8> {
    let mut b: Vec<u8> = (0..FACTORY_HEX.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&FACTORY_HEX[i..i + 2], 16).unwrap())
        .collect();
    b[774] = 0x31 + slot;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn factory() -> Profile {
        Profile {
            slot: 0,
            blob: factory_blob(0),
        }
    }

    #[test]
    fn factory_bank_matches_compiler() {
        let p = factory();
        let mut s = p.stick(0);
        let stored = s.bank;
        s.compile();
        assert_eq!(s.bank, stored);
        assert_eq!(stored, [50, 62, 75, 87, 100, 112, 125, 137, 150]);
    }

    #[test]
    fn deadzone_bank() {
        let mut s = factory().stick(0);
        s.center = 60;
        s.compile();
        assert_eq!(s.bank, [0, 0, 0, 0, 26, 56, 87, 118, 150]);
    }

    #[test]
    fn factory_title() {
        assert_eq!(factory().title(), "配置1");
    }

    #[test]
    fn macro_roundtrip() {
        let mut p = factory();
        let m = Macro {
            key: 18,
            kind: 2,
            interval_ms: Some(300),
            steps: vec![
                MacroStep {
                    delay_ms: 0,
                    key: 7,
                    press: true,
                },
                MacroStep {
                    delay_ms: 50,
                    key: 7,
                    press: false,
                },
            ],
        };
        p.set_macros(std::slice::from_ref(&m)).unwrap();
        assert_eq!(p.macros(), vec![m]);
        assert_eq!(p.key(18).target, TARGET_MACRO);
        p.set_macros(&[]).unwrap();
        assert!(p.macros().is_empty());
        assert_eq!(p.key(18).target, TARGET_IDENTITY);
    }

    #[test]
    fn edits_never_touch_vibration() {
        let orig = factory();
        let mut p = orig.clone();
        p.set_title("Test");
        p.set_travel(0, 10, 200);
        p.set_force(
            0,
            ForceEffect {
                mode: 1,
                params: [40, 100, 0, 0, 1, 0, 0, 0, 0, 0],
            },
        );
        p.set_force(
            1,
            ForceEffect {
                mode: 2,
                params: [0; 10],
            },
        ); // ignored
        assert_eq!(p.force(1).mode, 0);
        p.check_no_vibration_change(&orig).unwrap();
        let mut bad = orig.clone();
        bad.blob[147] = 0;
        assert!(bad.check_no_vibration_change(&orig).is_err());
    }

    #[test]
    fn grip_vibration_switches_off_and_back_on() {
        let on = factory();
        assert!(on.grip_vibration().master);
        let mut off = on.clone();
        off.set_grip_vibration(false);
        let g = off.grip_vibration();
        assert!(!g.master && !g.left && !g.right);
        off.check_no_vibration_change(&on).unwrap();
        let mut back = off.clone();
        back.set_grip_vibration(true);
        assert_eq!(back.blob, on.blob);
        back.check_no_vibration_change(&off).unwrap();
        // a switch may only be 0 or 0xFF
        let mut odd = on.clone();
        odd.blob[146] = 1;
        assert!(odd.check_no_vibration_change(&on).is_err());
        // the motor windows stay as they were
        assert_eq!(off.blob[147..150], on.blob[147..150]);
        assert_eq!(off.blob[151..183], on.blob[151..183]);
    }

    #[test]
    fn offset_and_ceiling_live_only_in_the_bank() {
        let mut p = factory();
        let mut s = p.stick(0);
        s.center = -20;
        s.edge = -30;
        p.set_stick(0, s);
        assert_eq!(p.blob[STICK_CORE[0] + 1], 0);
        assert_eq!(p.blob[STICK_EXTRA[0] + 11], 0);
        let back = p.stick(0);
        assert_eq!((back.center, back.edge), (-20, -30));
        assert_eq!(back.bank[0], 70);
        assert_eq!(back.bank[8], 120);
        assert!(
            back.bank.windows(2).all(|w| w[0] <= w[1]),
            "{:?}",
            back.bank
        );
        // the other stick is untouched
        assert_eq!(p.stick(1), factory().stick(1));
    }

    #[test]
    fn space_station_twos_complement_core_reads_from_the_bank() {
        let mut p = factory();
        let mut s = p.stick(1);
        s.center = -15;
        p.set_stick(1, s);
        p.blob[STICK_CORE[1] + 1] = (-15i8) as u8; // as Space Station writes it
        assert_eq!(p.stick(1).center, -15);
    }

    #[test]
    fn zones_keep_the_curve_rising() {
        let clamp = |center, edge| {
            let mut s = factory().stick(0);
            (s.center, s.edge) = (center, edge);
            s.clamp_zones();
            (s.center, s.edge)
        };
        assert_eq!(clamp(70, 50), (70, 30));
        assert_eq!(clamp(-70, -50), (-70, -30));
        assert_eq!(clamp(-70, 50), (-70, 50));
        assert_eq!(clamp(70, -50), (70, -50));
        assert_eq!(clamp(-120, 0), (-100, 0));
    }

    #[test]
    fn handles_map_back_to_their_points() {
        let mut s = factory().stick(0);
        (s.center, s.edge) = (-20, 10);
        for p in [(63, 63), (20, 100), (127, 127), (0, 0)] {
            s.p1 = p;
            let at = s.nodes()[1];
            let back = s.point_at(at.0, at.1);
            assert!(
                back.0.abs_diff(p.0) <= 1 && back.1.abs_diff(p.1) <= 1,
                "{p:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn a_copy_keeps_the_targets_vibration() {
        let target = factory();
        let mut source = factory();
        source.slot = 2;
        source.set_title("Шутер");
        source.set_grip_vibration(false);
        for o in [147, 151, 186, 206] {
            source.blob[o] ^= 0x5A; // strengths and trigger binding
        }
        source.blob[FORCE[0]] = 2; // recoil: vibration, never written
        source.blob[FORCE[1]] = 3; // a resistance comes along
        let p = source.onto(&target);
        assert_eq!(p.slot, 0);
        assert_eq!(p.title(), "Шутер");
        assert!(p.check_no_vibration_change(&target).is_ok());
        assert!(GRIP_SWITCHES.iter().all(|&o| p.blob[o] == 0xFF));
        assert_eq!(p.blob[147], target.blob[147]);
        assert_eq!(p.force(0).mode, target.force(0).mode);
        assert_eq!(p.force(1).mode, 3);
    }
}
