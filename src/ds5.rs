//! The DualSense this app can pretend to be, so games that speak DualSense
//! (No Man's Sky, through Steam Input) send their adaptive-trigger effects,
//! and how those effects become Apex 5 trigger modes.
//!
//! The report descriptor and feature reports were read off a real DualSense
//! by openflydigi (flydigi/ds5_usb.py), and the input report follows its
//! flydigi/ds5.py. Copyright (c) 2026 Mikalai Kaliaha, MIT licence, see
//! LICENSES/openflydigi-MIT.txt. The effect translation is this app's own.

use crate::evdev::Output;

pub const VENDOR: u32 = 0x054C;
pub const PRODUCT: u32 = 0x0CE6;
pub const NAME: &str = "Wireless Controller";
/// bcdDevice the kernel and Steam expect from a USB DualSense.
pub const VERSION: u32 = 0x8111;

/// HID report descriptor read off a real DualSense (289 bytes).
const REPORT_DESC: &str = concat!(
    "05010905a1018501093009310932093509330934150026ff0075089506810206",
    "00ff09209501810205010939150025073500463b016514750495018142650005",
    "091901290f150025017501950f81020600ff0921950d81020600ff0922150026",
    "ff0075089534810285020923952f9102850509339528b10285080934952fb102",
    "850909249513b102850a0925951ab102850b09419529b102850c09429529b102",
    "85200926953fb102852109279504b10285220940953fb10285800928953fb102",
    "85810929953fb1028582092a9509b1028583092b953fb1028584092c953fb102",
    "8585092d9502b10285a0092e9501b10285e0092f953fb10285f00930953fb102",
    "85f10931953fb10285f20932950fb10285f40935953fb10285f509369503b102",
    "c0",
);

/// Feature report 0x05, without its id (40 bytes).
const FEATURE_05: &str = concat!(
    "00000a00000092226edd8a228add8a2276dd1c021c020320f8dfe51fe7dff21f",
    "e4df0a0000000000",
);

/// Feature report 0x09, without its id (19 bytes).
const FEATURE_09: &str = "74e7d63a53350825001e00ee74d0bc00000000";

/// Feature report 0x0b, without its id (41 bytes).
const FEATURE_0B: &str = concat!(
    "74e7d63a5335082500011e00ee74d0bc001e00ee74d0bc000000000000000000",
    "000000000000000000",
);

/// Feature report 0x20, without its id (63 bytes).
const FEATURE_20: &str = concat!(
    "4a756c202034203230323531303a33383a343003000b00110800002a00100101",
    "48020000000000000000003006000001000300101003000600000000000000",
);
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

pub fn descriptor() -> Vec<u8> {
    unhex(REPORT_DESC)
}

/// Feature reports by number, each with its report id in front.
pub fn features() -> Vec<(u8, Vec<u8>)> {
    [
        (0x05, FEATURE_05),
        (0x09, FEATURE_09),
        (0x0B, FEATURE_0B),
        (0x20, FEATURE_20),
    ]
    .into_iter()
    .map(|(id, hex)| {
        let mut r = vec![id];
        r.extend(unhex(hex));
        (id, r)
    })
    .collect()
}

// ------------------------------------------------------------------ input

pub const REPORT_LEN: usize = 64;
const HAT_NEUTRAL: u8 = 8;
/// xpad has no digital L2/R2; past this much travel they count as pressed.
const TRIGGER_PRESSED: f32 = 0.12;
/// The Apex 5 accelerometer reads about 2.441 times less per g than the
/// DualSense calibration in FEATURE_05 assumes (openflydigi motion.py).
const ACCEL_SCALE: f32 = 2.441;

/// What the virtual pad reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    /// 0..255, 128 at rest, Y growing downwards.
    pub sticks: [(u8, u8); 2],
    pub l2: u8,
    pub r2: u8,
    pub hat: u8,
    pub buttons: [u8; 3],
    pub gyro: [i16; 3],
    pub accel: [i16; 3],
}

impl Default for Input {
    fn default() -> Self {
        Input {
            sticks: [(128, 128); 2],
            l2: 0,
            r2: 0,
            hat: HAT_NEUTRAL,
            buttons: [0; 3],
            gyro: [0; 3],
            accel: [0; 3],
        }
    }
}

/// (profile key id, button byte, mask). View is the touchpad click, since
/// games bind the same action there; View + Menu together is Create.
const BUTTONS: [(u8, usize, u8); 10] = [
    (7, 0, 0x10),  // X → square
    (4, 0, 0x20),  // A → cross
    (5, 0, 0x40),  // B → circle
    (8, 0, 0x80),  // Y → triangle
    (10, 1, 0x01), // LB → L1
    (11, 1, 0x02), // RB → R1
    (9, 1, 0x20),  // Menu → Options
    (14, 1, 0x40), // L3
    (15, 1, 0x80), // R3
    (27, 2, 0x01), // Home → PS
];
const VIEW: u8 = 6;
const MENU: u8 = 9;

impl Input {
    /// Buttons, sticks and triggers as games get them from the pad, after
    /// its profile; motion stays as it was.
    pub fn set_pad(&mut self, o: &Output) {
        let axis = |v: f32| ((v.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8;
        for (i, (x, y)) in o.sticks.iter().enumerate() {
            self.sticks[i] = (axis(*x), axis(-*y));
        }
        let pull = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        (self.l2, self.r2) = (pull(o.triggers.0), pull(o.triggers.1));
        let held = |id: u8| o.keys & (1 << id) != 0;
        let mut b = [0u8; 3];
        for (id, byte, mask) in BUTTONS {
            if held(id) {
                b[byte] |= mask;
            }
        }
        if o.triggers.0 > TRIGGER_PRESSED {
            b[1] |= 0x04;
        }
        if o.triggers.1 > TRIGGER_PRESSED {
            b[1] |= 0x08;
        }
        if held(VIEW) && held(MENU) {
            b[1] = (b[1] & !0x20) | 0x10; // Create, not Options
        } else if held(VIEW) {
            b[2] |= 0x02; // touchpad click
        }
        self.buttons = b;
        // ↑ 0, → 1, ↓ 2, ← 3
        let x = held(1) as i8 - held(3) as i8;
        let y = held(2) as i8 - held(0) as i8;
        self.hat = match (x, y) {
            (0, -1) => 0,
            (1, -1) => 1,
            (1, 0) => 2,
            (1, 1) => 3,
            (0, 1) => 4,
            (-1, 1) => 5,
            (-1, 0) => 6,
            (-1, -1) => 7,
            _ => HAT_NEUTRAL,
        };
    }

    /// Raw Apex 5 gyro and accelerometer from the vendor stream.
    pub fn set_motion(&mut self, gyro: [i16; 3], accel: [i16; 3]) {
        self.gyro = gyro;
        self.accel = accel.map(|v| (v as f32 * ACCEL_SCALE).clamp(-32768.0, 32767.0) as i16);
    }

    /// Nothing held, sticks centred: what the game gets while the pad is away.
    pub fn release(&mut self) {
        *self = Input::default();
    }

    /// USB input report 0x01. `timestamp` is in the DualSense's 1/3 µs.
    pub fn report(&self, seq: u8, timestamp: u32) -> [u8; REPORT_LEN] {
        let mut r = [0u8; REPORT_LEN];
        r[0] = 0x01;
        r[1] = self.sticks[0].0;
        r[2] = self.sticks[0].1;
        r[3] = self.sticks[1].0;
        r[4] = self.sticks[1].1;
        r[5] = self.l2;
        r[6] = self.r2;
        r[7] = seq;
        r[8] = (self.hat & 0x0F) | (self.buttons[0] & 0xF0);
        r[9] = self.buttons[1];
        r[10] = self.buttons[2];
        for i in 0..3 {
            r[16 + 2 * i..18 + 2 * i].copy_from_slice(&self.gyro[i].to_le_bytes());
            r[22 + 2 * i..24 + 2 * i].copy_from_slice(&self.accel[i].to_le_bytes());
        }
        r[28..32].copy_from_slice(&timestamp.to_le_bytes());
        // both touch points: bit 7 set = no contact
        r[33] = 0x80;
        r[37] = 0x80;
        // battery: full, not charging
        r[53] = 10 | (2 << 4);
        r[54] = 0x0C;
        r
    }
}

// ----------------------------------------------------------------- output

/// One trigger's effect as the game sent it: type and 10 parameter bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub kind: u8,
    pub params: [u8; 10],
}

#[derive(Debug, Default, PartialEq)]
pub struct Command {
    /// Grip motors (left = low frequency, right = high), when sent.
    pub rumble: Option<(u8, u8)>,
    /// Left and right trigger, when sent.
    pub triggers: [Option<Effect>; 2],
}

/// Decode an output report (USB 0x02 or Bluetooth 0x31).
pub fn parse_output(data: &[u8]) -> Command {
    let mut c = Command::default();
    let base = match data.first() {
        Some(0x02) => 1,
        Some(0x31) => 2,
        _ => return c,
    };
    if data.len() < base + 32 {
        return c;
    }
    let d = &data[base..];
    let flag2 = d.get(38).copied().unwrap_or(0);
    // motor (0x01) or "rumble, not haptics" (0x02), or compatible vibration
    if d[0] & 0x03 != 0 || flag2 & 0x04 != 0 {
        c.rumble = Some((d[3], d[2]));
    }
    let effect = |o: usize| Effect {
        kind: d[o],
        params: d[o + 1..o + 11].try_into().unwrap(),
    };
    if d[0] & 0x08 != 0 {
        c.triggers[0] = Some(effect(21));
    }
    if d[0] & 0x04 != 0 {
        c.triggers[1] = Some(effect(10));
    }
    c
}

/// An Apex 5 trigger mode and its parameters, as `device::engage_trigger`
/// takes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Apex {
    pub mode: u8,
    pub params: [u8; 10],
}

/// Trigger left alone.
pub const OFF: Apex = Apex {
    mode: 0,
    params: [0; 10],
};

/// Where DualSense zone 0..9 starts on the travel, 0..255.
fn pos(zone: u32) -> u32 {
    zone.min(10) * 255 / 10
}

/// The same as an Apex effect start, which goes up to 192.
fn at(zone: u32) -> u8 {
    pos(zone).min(192) as u8
}

/// A 1..8 strength over 1..255, on a square curve: the Apex's resistance
/// is already firm halfway up its range, so the low levels stay gentle
/// (4 of 8, where No Man's Sky starts a multitool's heat, gives 63).
fn force(level: u32) -> u8 {
    let l = level.clamp(1, 8);
    (l * l * 255 / 64).max(1) as u8
}

fn apex(mode: u8, p: &[u8]) -> Apex {
    let mut params = [0u8; 10];
    params[..p.len()].copy_from_slice(p);
    Apex { mode, params }
}

/// The DualSense effect the closest the Apex 5 can do, or `None` for one it
/// cannot (the trigger is then left as it is). Zone effects use the layouts
/// Sony's tools emit: a 10-bit zone mask, then 3 bits per zone.
pub fn translate(e: &Effect) -> Option<Apex> {
    let p = &e.params;
    let mask = u16::from_le_bytes([p[0], p[1]]) as u32 & 0x3FF;
    let packed = u32::from_le_bytes([p[2], p[3], p[4], p[5]]);
    let first = || mask.trailing_zeros();
    let last = || 31 - mask.leading_zeros();
    // strongest 1..8 level over the active zones
    let level = || {
        (0..10)
            .filter(|z| mask & (1 << z) != 0)
            .map(|z| ((packed >> (3 * z)) & 7) + 1)
            .max()
            .unwrap_or(0)
    };
    Some(match e.kind {
        0x00 | 0x05 => OFF,
        // Feedback: resistance from the first active zone on → Race
        0x21 if mask == 0 => OFF,
        0x21 => apex(1, &[at(first()), force(level())]),
        // Weapon: a wall between two zones that gives way → Recoil
        0x25 | 0x22 if mask.count_ones() >= 2 => {
            let strength = (p[2] & 7) as u32 + 1;
            let s = at(first());
            let band = (pos(last()) - s as u32).clamp(1, 255) as u8;
            apex(3, &[s, band, force(strength)])
        }
        // Vibration: shaking from the first active zone on → Sniper
        0x26 if mask == 0 => OFF,
        0x26 => apex(2, &[at(first()), 1, force(level()), p[8].max(1)]),
        // Galloping and Machine: rhythmic shaking between two zones
        0x23 | 0x27 if mask != 0 => apex(2, &[at(first()), 1, force(5), p[3].max(1)]),
        // the older simple forms: positions and strengths in 0..255
        0x01 => apex(1, &[p[0].min(192), p[1].max(1)]),
        0x02 => apex(
            3,
            &[p[0].min(192), p[1].saturating_sub(p[0]).max(1), p[2].max(1)],
        ),
        0x06 => apex(2, &[p[2].min(192), 1, p[1].max(1), p[0].max(1)]),
        _ => return None,
    })
}

/// Resistance (Race, Recoil) at `pct` % of its strength, never below 1;
/// shaking and the hard stop stay as they are.
pub fn scale_resistance(mut a: Apex, pct: u8) -> Apex {
    let i = match a.mode {
        1 => 1,
        3 => 2,
        _ => return a,
    };
    let v = a.params[i] as u32 * pct.min(100) as u32 / 100;
    a.params[i] = v.max(1) as u8;
    a
}

/// The game's effect in words.
pub fn describe(e: &Effect) -> String {
    let p = &e.params;
    let mask = u16::from_le_bytes([p[0], p[1]]) & 0x3FF;
    let packed = u32::from_le_bytes([p[2], p[3], p[4], p[5]]);
    let level = (0..10)
        .filter(|z| mask & (1 << z) != 0)
        .map(|z| ((packed >> (3 * z)) & 7) + 1)
        .max()
        .unwrap_or(0);
    match e.kind {
        0x00 | 0x05 => "нет эффекта".into(),
        0x21 => format!("сопротивление {level} из 8"),
        0x25 => format!("упор, сила {} из 8", (p[2] & 7) + 1),
        0x22 => "тетива лука".into(),
        0x26 => format!("вибрация {level} из 8, {} Гц", p[8]),
        0x23 => "галоп".into(),
        0x27 => "очередь".into(),
        0x01 => "сопротивление".into(),
        0x02 => "упор".into(),
        0x06 => "вибрация".into(),
        k => format!("эффект 0x{k:02x}"),
    }
}

/// An Apex trigger mode in words.
pub fn describe_apex(a: &Apex) -> String {
    let p = &a.params;
    match a.mode {
        1 => format!("Гоночный, сила {}", p[1]),
        2 => format!("Очередь, сила {}, частота {}", p[2], p[3]),
        3 => format!("Прорыв, сила {}", p[2]),
        4 => "Блокировка".into(),
        _ => "Обычный".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(kind: u8, hex: &str) -> Effect {
        Effect {
            kind,
            params: unhex(&hex.replace(' ', "")).try_into().unwrap(),
        }
    }

    #[test]
    fn data_has_the_hardware_sizes() {
        assert_eq!(descriptor().len(), 289);
        let f = features();
        assert_eq!(f.iter().find(|(id, _)| *id == 0x05).unwrap().1.len(), 41);
        assert!(f.iter().all(|(id, r)| r[0] == *id));
    }

    // effects No Man's Sky sent through Steam, captured 2026-10-10
    #[test]
    fn heat_grows_the_resistance() {
        let heat = |hex| translate(&effect(0x21, hex)).unwrap();
        let cold = heat("ff 03 00 00 00 00 00 00 00 00");
        let warm = heat("ff 03 db b6 6d 1b 00 00 00 00");
        let warmer = heat("ff 03 24 49 92 24 00 00 00 00");
        let hot = heat("ff 03 6d db b6 2d 00 00 00 00");
        for a in [cold, warm, warmer, hot] {
            assert_eq!(a.mode, 1);
            assert_eq!(a.params[0], 0, "from the start of the travel");
        }
        assert_eq!(
            [cold, warm, warmer, hot].map(|a| a.params[1]),
            [3, 63, 99, 143]
        );
    }

    #[test]
    fn resistance_scales_and_shaking_does_not() {
        let race = apex(1, &[0, 143]);
        assert_eq!(scale_resistance(race, 60).params[1], 85);
        assert_eq!(scale_resistance(race, 0).params[1], 1);
        assert_eq!(scale_resistance(race, 100), race);
        let recoil = apex(3, &[102, 102, 255]);
        assert_eq!(
            scale_resistance(recoil, 50).params,
            apex(3, &[102, 102, 127]).params
        );
        let shake = apex(2, &[0, 1, 255, 76]);
        assert_eq!(scale_resistance(shake, 10), shake);
    }

    #[test]
    fn a_weapon_wall_is_a_recoil() {
        let a = translate(&effect(0x25, "10 01 07 00 00 00 00 00 00 00")).unwrap();
        assert_eq!(a, apex(3, &[102, 102, 255]), "zones 4..8, strength 8");
    }

    #[test]
    fn vibration_keeps_the_games_frequency() {
        let a = translate(&effect(0x26, "ff 03 ff ff ff 3f 00 00 4c 00")).unwrap();
        assert_eq!(a, apex(2, &[0, 1, 255, 76]));
    }

    #[test]
    fn off_and_unknown() {
        assert_eq!(
            translate(&effect(0x05, "00 00 00 00 00 00 00 00 00 00")),
            Some(OFF)
        );
        assert_eq!(
            translate(&effect(0x21, "00 00 00 00 00 00 00 00 00 00")),
            Some(OFF)
        );
        assert_eq!(
            translate(&effect(0x77, "00 00 00 00 00 00 00 00 00 00")),
            None
        );
    }

    #[test]
    fn output_report_carries_both_triggers() {
        // the first Weapon report, as Steam wrote it
        let raw = unhex(concat!(
            "020c0000000000000000002510010700000000000000050000000000000000",
            "0000000000000000000000000000000000",
        ));
        let c = parse_output(&raw);
        assert_eq!(c.triggers[1].unwrap().kind, 0x25);
        assert_eq!(c.triggers[0].unwrap().kind, 0x05);
        assert_eq!(c.rumble, None);
    }

    #[test]
    fn pad_buttons_land_where_a_dualsense_has_them() {
        let mut i = Input::default();
        let o = Output {
            sticks: [(1.0, 1.0), (0.0, 0.0)],
            triggers: (0.5, 0.0),
            keys: 1 << 4 | 1 << 0 | 1 << 1 | 1 << 6,
        };
        i.set_pad(&o);
        assert_eq!(i.sticks[0], (255, 0), "up is 0");
        assert_eq!(i.sticks[1], (128, 128));
        assert_eq!(i.hat, 1, "↑ + → = north-east");
        assert_eq!(i.buttons, [0x20, 0x04, 0x02], "cross, L2, touchpad");
        let r = i.report(5, 0);
        assert_eq!((r[0], r[7], r[8]), (0x01, 5, 0x21));
    }
}
