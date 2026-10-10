//! Last line of defence: every packet is checked here right before it is
//! written to the pad. Anything not explicitly allowed is refused.
//!
//! Grip rumble (0x12) only in SDL's form for the two grip motors, for the
//! rumble test. Never allowed: rumble-to-trigger bind (82), vibrating
//! trigger modes (81 mode 2/5), firmware/upgrade mode (31; the one way past
//! it is `SCREEN_UPLOAD_MODE` below), factory reset (253),
//! reset-all-profiles (175), mode switch (27), restart (29), report
//! rate (20), test floods (242/245), the HID picture family (208-211), the
//! Switch-bank commit (171), third-party capture (28), the factory/test range
//! (240-253) and the device-type rewrite (254).

use anyhow::{Result, bail};

/// Command 31 with chip 4 (the screen), the one way into a picture upload.
/// `check` still refuses it: it is sent only by `Device::switch_screen`,
/// after `device::enter_screen_upload` has seen the cable.
pub const SCREEN_UPLOAD_MODE: [u8; 7] = [0x03, 0x5A, 0xA5, 0x1F, 0x03, 0x04, 0x26];

/// Sub-ids of command 19 the app may toggle (2 = Xbox home, 3/10 unsupported).
/// 4 is Space Station's "Turbo Function".
pub const ALLOWED_SUB19: [u8; 7] = [1, 4, 5, 6, 7, 8, 9];

/// Force-trigger modes that only resist: Normal, Race, Recoil(UI "Sniper"), Lock.
pub const RESISTANCE_MODES: [u8; 4] = [0, 1, 3, 4];

pub fn check(pkt: &[u8; 32]) -> Result<()> {
    let cmd = pkt[3];
    let p = &pkt[5..];
    match cmd {
        // reads: info, nickname, settings status, uid, transport flags
        1 | 2 | 3 | 4 | 16 => Ok(()),
        // transport flags: one at a time, the raw-data stream (p[1]) or
        // letting third-party apps take the pad over (p[4]); the taking
        // over itself (28) stays blocked
        17 => {
            let one = (p[1] <= 1 && p[4] == 0xFF) || (p[1] == 0xFF && p[4] <= 1);
            if p[0] != 0xFF || p[2] != 0xFF || p[3] != 0xFF || !one {
                bail!("blocked: command 17 may only toggle the stream or third-party control");
            }
            Ok(())
        }
        19 if ALLOWED_SUB19.contains(&p[0]) && p[1] <= 1 => Ok(()),
        19 => bail!("blocked: setting sub-id {} is not allowed", p[0]),
        21 if (1..=7).contains(&p[0]) => Ok(()),
        22 if (14..=20).contains(&p[0]) => Ok(()),
        // sleep minutes: Space Station's picker offers 0 (never), 1, 5, 15, 60, 180
        23 if p[0] <= 60 || p[0] == 180 => Ok(()),
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
        // grip rumble test: SDL's packet, length 6, no checksum; the trigger
        // motor bytes stay 0 (the Apex 5 has none)
        0x12 if pkt[4] == 6 && p[2] == 0 && p[3] == 0 => Ok(()),
        0x12 => bail!("blocked: rumble may only drive the two grip motors"),
        82 => bail!("blocked: vibration command {cmd}"),
        // Space Station sends 254 when its server says so, then restarts the
        // pad; the model id lives in flash and both apps key off it.
        254 => bail!("blocked: device-type rewrite (254) changes the pad's model id"),
        240..=253 => bail!("blocked: factory/test command {cmd}"),
        _ => bail!("blocked: command {cmd} is not on the allow-list"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::build;

    #[test]
    fn rumble_reaches_only_the_grip_motors() {
        let rumble = |l, r, lt, rt| {
            let mut pkt = build(0x12, &[l, r, lt, rt], false);
            pkt[4] = 6;
            pkt
        };
        assert!(check(&rumble(255, 128, 0, 0)).is_ok());
        assert!(check(&rumble(0, 0, 0, 0)).is_ok());
        assert!(check(&rumble(0, 0, 255, 0)).is_err());
        assert!(check(&rumble(0, 0, 0, 1)).is_err());
        assert!(check(&build(0x12, &[0, 0, 0, 0], false)).is_err()); // length 4
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
        for cmd in [
            20, 27, 28, 29, 31, 171, 175, 208, 211, 240, 241, 242, 244, 245, 246, 247, 253, 254,
        ] {
            assert!(check(&build(cmd, &[4], true)).is_err(), "cmd {cmd}");
        }
        assert!(check(&build(19, &[2, 1], true)).is_err());
        assert!(check(&build(17, &[0, 1, 0xFF, 0xFF, 0xFF], true)).is_err());
        assert!(check(&build(17, &[0xFF, 1, 0xFF, 0xFF, 0xFF], true)).is_ok());
    }

    #[test]
    fn the_screen_switch_is_command_31_for_chip_4_only() {
        let mut pkt = [0u8; 32];
        pkt[..7].copy_from_slice(&SCREEN_UPLOAD_MODE);
        assert_eq!(pkt, build(31, &[4], true));
        assert!(check(&pkt).is_err(), "never through the ordinary path");
    }

    #[test]
    fn device_type_rewrite_is_refused_for_every_type() {
        for t in [0, 128, 129, 133, 136, 255] {
            let err = check(&build(254, &[t], true)).unwrap_err().to_string();
            assert!(err.contains("device-type"), "{err}");
        }
    }

    #[test]
    fn transport_flags_change_one_at_a_time() {
        let flags = |p: [u8; 5]| check(&build(17, &p, true));
        assert!(flags([0xFF, 1, 0xFF, 0xFF, 0xFF]).is_ok(), "stream on");
        assert!(flags([0xFF, 0, 0xFF, 0xFF, 0xFF]).is_ok(), "stream off");
        assert!(flags([0xFF, 0xFF, 0xFF, 0xFF, 1]).is_ok(), "third party on");
        assert!(
            flags([0xFF, 0xFF, 0xFF, 0xFF, 0]).is_ok(),
            "third party off"
        );
        assert!(flags([0, 0xFF, 0xFF, 0xFF, 0xFF]).is_err(), "xinput");
        assert!(flags([0xFF, 0xFF, 1, 0xFF, 0xFF]).is_err(), "keyboard");
        assert!(flags([0xFF, 0xFF, 0xFF, 1, 0xFF]).is_err(), "mouse");
        assert!(flags([0xFF, 1, 0xFF, 0xFF, 1]).is_err(), "two at once");
        assert!(flags([0xFF, 0xFF, 0xFF, 0xFF, 2]).is_err());
        assert!(flags([0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).is_err(), "nothing");
    }

    #[test]
    fn sleep_allows_up_to_an_hour_or_exactly_three() {
        for v in [0, 1, 5, 15, 60, 180] {
            assert!(check(&build(23, &[v], true)).is_ok(), "sleep {v}");
        }
        for v in [61, 179, 181, 255] {
            assert!(check(&build(23, &[v], true)).is_err(), "sleep {v}");
        }
    }
}
