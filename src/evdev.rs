//! The pad's gamepad node as games see it (xpad), read only for the input
//! tester: the sticks after the profile's zones, curve and circle.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;
pub const EV_ABS: u16 = 3;
/// ABS_X, ABS_Y, ABS_RX, ABS_RY.
const AXES: [u16; 4] = [0, 1, 3, 4];

/// `struct input_event` on 64-bit Linux.
const EVENT_LEN: usize = 24;

/// EVIOCGABS(code): `_IOR('E', 0x40 + code, struct input_absinfo)`.
fn eviocgabs(code: u16) -> libc::c_ulong {
    (2 << 30) | (24 << 16) | ((b'E' as libc::c_ulong) << 8) | (0x40 + code as libc::c_ulong)
}

/// The event node on the same USB device as the vendor hidraw node that
/// carries both sticks.
pub fn find(hidraw: &Path) -> Result<PathBuf> {
    let name = hidraw.file_name().context("hidraw path")?;
    let iface = std::fs::canonicalize(Path::new("/sys/class/hidraw").join(name).join("device"))?;
    // hid device → USB interface → USB device
    let usb = iface
        .parent()
        .and_then(Path::parent)
        .context("hidraw has no USB parent")?;
    let mut nodes: Vec<_> = std::fs::read_dir("/sys/class/input")?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
        .collect();
    nodes.sort_by_key(|e| e.file_name());
    for e in nodes {
        let Ok(dev) = std::fs::canonicalize(e.path().join("device")) else {
            continue;
        };
        if !dev.starts_with(usb) {
            continue;
        }
        let abs =
            std::fs::read_to_string(e.path().join("device/capabilities/abs")).unwrap_or_default();
        let bits = abs
            .split_whitespace()
            .last()
            .and_then(|w| u64::from_str_radix(w, 16).ok())
            .unwrap_or(0);
        if AXES.iter().all(|a| bits & (1 << a) != 0) {
            return Ok(PathBuf::from("/dev/input").join(e.file_name()));
        }
    }
    bail!("no gamepad event node next to {}", hidraw.display())
}

/// One `struct input_event`.
#[derive(Clone, Copy, Debug)]
pub struct Event {
    /// Kernel timestamp in seconds.
    pub time: f64,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

fn event(ev: &[u8; EVENT_LEN]) -> Event {
    let secs = i64::from_ne_bytes(ev[0..8].try_into().unwrap());
    let usecs = i64::from_ne_bytes(ev[8..16].try_into().unwrap());
    Event {
        time: secs as f64 + usecs as f64 / 1e6,
        kind: u16::from_ne_bytes([ev[16], ev[17]]),
        code: u16::from_ne_bytes([ev[18], ev[19]]),
        value: i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]),
    }
}

pub struct Pad {
    file: File,
    range: [(i32, i32); 4],
    raw: [i32; 4],
}

impl Pad {
    pub fn open(path: &Path) -> Result<Pad> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .with_context(|| format!("open {}", path.display()))?;
        let mut range = [(0, 0); 4];
        let mut raw = [0; 4];
        for (i, a) in AXES.iter().enumerate() {
            // value, minimum, maximum, fuzz, flat, resolution
            let mut info = [0i32; 6];
            // SAFETY: EVIOCGABS fills a 24-byte input_absinfo.
            if unsafe { libc::ioctl(file.as_raw_fd(), eviocgabs(*a), info.as_mut_ptr()) } < 0 {
                bail!("EVIOCGABS on {}", path.display());
            }
            range[i] = (info[1], info[2]);
            raw[i] = info[0];
        }
        Ok(Pad { file, range, raw })
    }

    /// Everything queued on the node, oldest first.
    pub fn events(&mut self) -> Result<Vec<Event>> {
        let mut out = Vec::new();
        let mut buf = [0u8; EVENT_LEN * 64];
        loop {
            let n = match self.file.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e).context("gamepad node read failed"),
            };
            if n == 0 {
                break;
            }
            out.extend(buf[..n].as_chunks::<EVENT_LEN>().0.iter().map(event));
        }
        Ok(out)
    }

    /// Reads what is queued and returns the stick positions at every
    /// report, in -1..1 with +Y up: `[(lx, ly), (rx, ry)]`.
    pub fn read(&mut self) -> Result<Vec<[(f32, f32); 2]>> {
        let mut out = Vec::new();
        for ev in self.events()? {
            match ev.kind {
                EV_ABS => {
                    if let Some(i) = AXES.iter().position(|a| *a == ev.code) {
                        self.raw[i] = ev.value;
                    }
                }
                EV_SYN if ev.code == 0 => out.push(self.sticks()),
                _ => {}
            }
        }
        Ok(out)
    }

    pub fn sticks(&self) -> [(f32, f32); 2] {
        let v = |i: usize| norm(self.raw[i], self.range[i]);
        // xpad reports Y growing downwards
        [(v(0), -v(1)), (v(2), -v(3))]
    }
}

fn norm(v: i32, (min, max): (i32, i32)) -> f32 {
    if max <= min {
        return 0.0;
    }
    let mid = (min as f32 + max as f32) / 2.0;
    ((v as f32 - mid) / ((max - min) as f32 / 2.0)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_info_ioctl_number() {
        // EVIOCGABS(ABS_X) as the kernel headers spell it
        assert_eq!(eviocgabs(0), 0x8018_4540);
        assert_eq!(eviocgabs(4), 0x8018_4544);
    }

    #[test]
    fn xpad_range_maps_to_unit() {
        let r = (-32768, 32767);
        assert_eq!(norm(32767, r), 1.0);
        assert_eq!(norm(-32768, r), -1.0);
        assert!(norm(0, r).abs() < 0.001);
    }
}
