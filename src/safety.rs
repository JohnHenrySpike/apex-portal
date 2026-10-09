//! Last line of defence: every packet is checked here right before it is
//! written to the pad. Anything not explicitly allowed is refused.
//!
//! Never allowed: rumble (0x12), rumble-to-trigger bind (82), vibrating
//! trigger modes (81 mode 2/5), firmware/upgrade mode (31), factory reset
//! (253), reset-all-profiles (175), mode switch (27), restart (29), report
//! rate (20), test floods (242/245), the HID picture family (208-211), and
//! the Switch-bank commit (171).

use anyhow::{Result, bail};

/// Sub-ids of command 19 the app may toggle (2 = Xbox home, 3/10 unsupported).
pub const ALLOWED_SUB19: [u8; 7] = [1, 4, 5, 6, 7, 8, 9];

/// Force-trigger modes that only resist: Normal, Race, Recoil(UI "Sniper"), Lock.
pub const RESISTANCE_MODES: [u8; 4] = [0, 1, 3, 4];

pub fn check(pkt: &[u8; 32]) -> Result<()> {
    let cmd = pkt[3];
    let p = &pkt[5..];
    match cmd {
        // reads: info, nickname, settings status, uid, transport flags
        1 | 2 | 3 | 4 | 16 => Ok(()),
        // transport flags: only the raw-data stream flag may change
        17 => {
            if p[0] != 0xFF || p[2] != 0xFF || p[3] != 0xFF || p[4] != 0xFF || p[1] > 1 {
                bail!("blocked: command 17 may only toggle the input stream");
            }
            Ok(())
        }
        19 if ALLOWED_SUB19.contains(&p[0]) && p[1] <= 1 => Ok(()),
        19 => bail!("blocked: setting sub-id {} is not allowed", p[0]),
        21 if (1..=7).contains(&p[0]) => Ok(()),
        22 if (14..=20).contains(&p[0]) => Ok(()),
        23 if p[0] <= 60 => Ok(()),
        21..=23 => bail!("blocked: setting {cmd} value {} out of range", p[0]),
        24 => Ok(()), // nickname
        // live force trigger: one side at a time, resistance modes only
        81 => {
            if p[0] != 1 || !(p[1] == 1 || p[1] == 2) || !RESISTANCE_MODES.contains(&p[2]) {
                bail!("blocked: trigger mode {} vibrates or is malformed", p[2]);
            }
            Ok(())
        }
        // mapping profile: status, apply, read, write, save
        161..=166 => Ok(()),
        // lighting: read, write
        167..=169 => Ok(()),
        0x12 | 82 => bail!("blocked: vibration command {cmd}"),
        _ => bail!("blocked: command {cmd} is not on the allow-list"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::build;

    #[test]
    fn rumble_is_refused() {
        assert!(check(&build(0x12, &[0, 0, 0, 0], false)).is_err());
        assert!(check(&build(82, &[1, 2, 255, 0, 0, 0, 0, 0], false)).is_err());
    }

    #[test]
    fn vibrating_trigger_modes_are_refused() {
        assert!(check(&build(81, &[1, 1, 2, 0, 1, 1, 1, 0], false)).is_err());
        assert!(check(&build(81, &[1, 2, 5, 0, 1, 1, 1, 0], false)).is_err());
        assert!(check(&build(81, &[1, 3, 1, 0, 100, 0], false)).is_err()); // side Both
        assert!(check(&build(81, &[1, 1, 1, 0, 100, 0], false)).is_ok());
    }

    #[test]
    fn dangerous_commands_are_refused() {
        for cmd in [20, 27, 29, 31, 171, 175, 208, 211, 242, 245, 253] {
            assert!(check(&build(cmd, &[4], true)).is_err(), "cmd {cmd}");
        }
        assert!(check(&build(19, &[2, 1], true)).is_err());
        assert!(check(&build(17, &[0, 1, 0xFF, 0xFF, 0xFF], true)).is_err());
        assert!(check(&build(17, &[0xFF, 1, 0xFF, 0xFF, 0xFF], true)).is_ok());
    }
}
