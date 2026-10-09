//! Identity, device settings, nickname, live input stream and live
//! (resistance-only) trigger effects. Offsets are raw: report id = byte 0.

use anyhow::{Result, anyhow, bail};
use std::time::Duration;

use crate::hid::{Device, Reply, build};

const T: Duration = Duration::from_millis(600);

fn need(r: Option<Reply>, what: &str) -> Result<Reply> {
    r.ok_or_else(|| anyhow!("no reply to {what} (the pad may be asleep — press a button)"))
}

// ---------------------------------------------------------------- identity

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub device_type: u8,
    pub wired: bool,
    pub battery_level: u8, // 0..5
    pub charging: bool,
    pub firmware: Vec<(&'static str, String)>,
    pub uid: String,
    pub nickname: String,
}

impl Info {
    pub fn model(&self) -> &'static str {
        match self.device_type {
            128 => "Apex 5",
            129 => "Apex 5 EVA",
            133 => "Apex 5 MM",
            134 => "Apex 5 SRS",
            135 => "Apex 5 GS",
            136 => "Apex 5 LZ",
            _ => "unknown",
        }
    }
    pub fn supported(&self) -> bool {
        matches!(self.device_type, 128 | 129 | 133..=136)
    }
}

fn version(hi: u8, lo: u8) -> Option<String> {
    (hi != 0 || lo != 0).then(|| format!("{}.{}.{}.{}", hi >> 4, hi & 15, lo >> 4, lo & 15))
}

pub fn read_info(dev: &mut Device) -> Result<Info> {
    let r = need(dev.query(1, &[], T)?, "get-info")?;
    let names = [
        "main", "dongle", "switch", "trigger", "screen", "ADC", "NearLink",
    ];
    let firmware = names
        .iter()
        .enumerate()
        .filter_map(|(i, n)| version(r.at(16 + 2 * i), r.at(17 + 2 * i)).map(|v| (*n, v)))
        .collect();
    let bat = r.at(12);
    let mut info = Info {
        device_type: r.at(6),
        wired: r.at(7) == 1,
        battery_level: (bat & 0x0F).min(5),
        charging: bat >> 4 == 1,
        firmware,
        ..Default::default()
    };
    if let Some(u) = dev.query(4, &[], T)? {
        info.uid = (6..19).map(|i| format!("{:02x}", u.at(i))).collect();
    }
    info.nickname = read_nickname(dev).unwrap_or_default();
    Ok(info)
}

pub fn read_nickname(dev: &mut Device) -> Result<String> {
    let r = need(dev.query(2, &[], T)?, "nickname")?;
    let bytes: Vec<u8> = r.0[5..]
        .iter()
        .copied()
        .take_while(|b| *b != 0 && *b != 0xFF)
        .collect();
    match String::from_utf8(bytes) {
        Ok(s) if !s.is_empty() && s.chars().all(|c| !c.is_control()) => Ok(s),
        _ => Ok(String::new()),
    }
}

pub const NICKNAME_MAX: usize = 26;

pub fn write_nickname(dev: &mut Device, name: &str) -> Result<()> {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > NICKNAME_MAX {
        bail!("name must be 1..{NICKNAME_MAX} bytes of UTF-8");
    }
    // No checksum: the pad stores [4]-1 bytes from [5], so the byte after the
    // name must stay 0.
    let mut pkt = build(24, b, false);
    pkt[4] = (b.len() + 2) as u8;
    dev.request(pkt, T, |_| true)?;
    if read_nickname(dev)? != name {
        bail!("the pad did not store the new name");
    }
    Ok(())
}

// ---------------------------------------------------------------- settings

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub supported: u16,
    pub enabled: u16,
    pub sleep_minutes: u8,
    pub report_rate: u8,
    pub precision: u8,
    pub sensitivity: u8,
}

impl Settings {
    pub fn is_supported(&self, sub: u8) -> bool {
        self.supported & (1 << (sub - 1)) != 0
    }
    pub fn is_on(&self, sub: u8) -> bool {
        self.enabled & (1 << (sub - 1)) != 0
    }
}

pub fn read_settings(dev: &mut Device) -> Result<Settings> {
    let r = need(dev.query(3, &[], T)?, "settings")?;
    Ok(Settings {
        supported: r.at(6) as u16 | ((r.at(8) as u16) << 8),
        enabled: r.at(7) as u16 | ((r.at(9) as u16) << 8),
        sleep_minutes: r.at(10),
        report_rate: r.at(11),
        precision: r.at(12),
        sensitivity: r.at(13),
    })
}

pub fn set_feature(dev: &mut Device, sub: u8, on: bool) -> Result<()> {
    need(dev.query(19, &[sub, on as u8], T)?, "setting write")?;
    Ok(())
}

/// 21 = stick precision, 22 = centre sensitivity, 23 = sleep minutes.
pub fn set_number(dev: &mut Device, cmd: u8, value: u8) -> Result<()> {
    need(dev.query(cmd, &[value], T)?, "setting write")?;
    Ok(())
}

pub const PRECISION: [(u8, &str); 7] = [
    (1, "8 бит"),
    (4, "9 бит"),
    (2, "10 бит"),
    (5, "11 бит"),
    (3, "12 бит"),
    (6, "14 бит"),
    (7, "16 бит"),
];
/// Space Station's choices; `safety` refuses anything else above an hour.
pub const SLEEP: [(u8, &str); 6] = [
    (1, "1 мин"),
    (5, "5 мин"),
    (15, "15 мин"),
    (60, "1 час"),
    (180, "3 часа"),
    (0, "Никогда"),
];
pub const SENSITIVITY: [(u8, &str); 7] = [
    (14, "Максимальная"),
    (15, "Высокая"),
    (16, "Выше средней"),
    (17, "Средняя"),
    (18, "Ниже средней"),
    (19, "Низкая"),
    (20, "Минимальная"),
];

// ------------------------------------------------------------ input stream

#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    pub controller_data: bool,
    pub raw_data: bool,
}

pub fn read_flags(dev: &mut Device) -> Result<Flags> {
    let r = need(dev.query(16, &[], T)?, "transport flags")?;
    Ok(Flags {
        controller_data: r.at(6) == 1,
        raw_data: r.at(7) == 1,
    })
}

pub fn set_stream(dev: &mut Device, on: bool) -> Result<()> {
    dev.query(17, &[0xFF, on as u8, 0xFF, 0xFF, 0xFF], T)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InputState {
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
    pub buttons: [u8; 4],
    pub lt: u8,
    pub rt: u8,
    pub gyro: [i16; 3],
    pub accel: [i16; 3],
}

impl InputState {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 30 || raw[3] != crate::hid::INPUT_REPORT_CMD {
            return None;
        }
        let i = |o: usize| i16::from_le_bytes([raw[o], raw[o + 1]]);
        Some(InputState {
            lx: i(4),
            ly: i(6),
            rx: i(8),
            ry: i(10),
            buttons: [raw[12], raw[13], raw[14], raw[15]],
            lt: raw[16],
            rt: raw[17],
            gyro: [i(18), i(20), i(22)],
            accel: [i(24), i(26), i(28)],
        })
    }
    pub fn pressed(&self, byte: usize, mask: u8) -> bool {
        self.buttons[byte] & mask != 0
    }
}

/// (label, byte, mask) in the 0xEF report.
pub const BUTTONS: [(&str, usize, u8); 27] = [
    ("↑", 0, 0x01),
    ("→", 0, 0x02),
    ("↓", 0, 0x04),
    ("←", 0, 0x08),
    ("A", 0, 0x10),
    ("B", 0, 0x20),
    ("View", 0, 0x40),
    ("X", 0, 0x80),
    ("Y", 1, 0x01),
    ("Menu", 1, 0x02),
    ("LB", 1, 0x04),
    ("RB", 1, 0x08),
    ("LT", 1, 0x10),
    ("RT", 1, 0x20),
    ("L3", 1, 0x40),
    ("R3", 1, 0x80),
    ("M1", 2, 0x04),
    ("M2", 2, 0x08),
    ("M3", 2, 0x10),
    ("M4", 2, 0x20),
    ("LM", 2, 0x40),
    ("RM", 2, 0x80),
    ("Fn", 3, 0x01),
    ("Turbo", 3, 0x02),
    ("Home", 3, 0x08),
    ("Back", 3, 0x10),
    ("?", 3, 0x04),
];

// ------------------------------------------------ live trigger resistance

pub const SIDE_LEFT: u8 = 1;
pub const SIDE_RIGHT: u8 = 2;

/// Send one trigger's effect live (command 81, no checksum, [4]=10).
/// Vibrating modes are mapped to Normal here and refused again by `safety`.
pub fn engage_trigger(dev: &mut Device, side: u8, mode: u8, p: &[u8; 10]) -> Result<()> {
    let at_least_one = |v: u8| v.max(1);
    let payload: Vec<u8> = match mode {
        1 => vec![1, side, 1, p[0], at_least_one(p[1]), (p[4] == 1) as u8],
        3 => vec![
            1,
            side,
            3,
            p[0],
            p[1],
            at_least_one(p[2]),
            0,
            (p[4] == 1) as u8,
        ],
        4 => vec![1, side, 4, p[0], 255, 1],
        _ => vec![1, side, 0],
    };
    let mut pkt = build(81, &payload, false);
    pkt[4] = 10;
    dev.request(pkt, T, |_| true)?;
    Ok(())
}
