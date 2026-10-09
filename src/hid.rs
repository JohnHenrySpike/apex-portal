//! Vendor hidraw transport for the Apex 5 (HID interface 2, usage page 0xFFA0).
//!
//! Every outgoing command passes `safety::check` first: vibration and anything
//! that can touch firmware is refused here, below every caller.

use anyhow::{Context, Result, anyhow, bail};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::safety;

pub const VID: &str = "000037D7";
pub const PID_APEX5: &str = "00002501";
const VENDOR_DESCRIPTOR_PREFIX: [u8; 3] = [0x06, 0xa0, 0xff];

pub const REPORT_OUT: u8 = 0x03;
pub const REPORT_IN: u8 = 0x04;
pub const MAGIC: [u8; 2] = [0x5A, 0xA5];
pub const PACKET_LEN: usize = 32;
/// Marks a streamed input report rather than a command reply.
pub const INPUT_REPORT_CMD: u8 = 0xEF;

/// Find the vendor hidraw node of a connected Apex 5.
pub fn find_device() -> Result<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir("/sys/class/hidraw")
        .context("no /sys/class/hidraw")?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let dir = e.path().join("device");
        let Ok(uevent) = std::fs::read_to_string(dir.join("uevent")) else {
            continue;
        };
        if !uevent.contains(VID) {
            continue;
        }
        let Ok(desc) = std::fs::read(dir.join("report_descriptor")) else {
            continue;
        };
        if desc.starts_with(&VENDOR_DESCRIPTOR_PREFIX) {
            return Ok(PathBuf::from("/dev").join(e.file_name()));
        }
    }
    bail!("Flydigi Apex 5 not found (connect it by cable or dongle)")
}

/// Envelope builder. `checksummed` = length byte is payload+2 and an 8-bit sum
/// over [3, 3+len) follows; otherwise length is the raw payload length.
pub fn build(cmd: u8, payload: &[u8], checksummed: bool) -> [u8; PACKET_LEN] {
    let mut buf = [0u8; PACKET_LEN];
    buf[0] = REPORT_OUT;
    buf[1..3].copy_from_slice(&MAGIC);
    buf[3] = cmd;
    buf[5..5 + payload.len()].copy_from_slice(payload);
    if checksummed {
        let len = payload.len() + 2;
        buf[4] = len as u8;
        buf[3 + len] = checksum(&buf[3..3 + len]);
    } else {
        buf[4] = payload.len() as u8;
    }
    buf
}

pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
}

/// A reply with the report-id byte stripped: `[5A, A5, cmd, ...]`.
#[derive(Clone, Debug)]
pub struct Reply(pub Vec<u8>);

impl Reply {
    pub fn cmd(&self) -> u8 {
        self.0[2]
    }
    /// Byte at an offset counted *with* the report id, as the protocol notes do.
    pub fn at(&self, offset_with_report_id: usize) -> u8 {
        self.0.get(offset_with_report_id - 1).copied().unwrap_or(0)
    }
}

pub struct Device {
    file: File,
    pub path: PathBuf,
    /// Input reports seen while waiting for replies, for the live tester.
    pub on_input: Option<Box<dyn FnMut(&[u8]) + Send>>,
}

impl Device {
    pub fn open() -> Result<Self> {
        let path = find_device()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .with_context(|| {
                format!(
                    "cannot open {} (install the udev rule, see README)",
                    path.display()
                )
            })?;
        let mut dev = Device {
            file,
            path,
            on_input: None,
        };
        dev.drain();
        Ok(dev)
    }

    fn poll(&self, timeout: Duration) -> bool {
        let mut pfd = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        unsafe { libc::poll(&mut pfd, 1, ms) > 0 }
    }

    /// Read one raw report if any is waiting within `timeout`.
    pub fn read_raw(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        if !self.poll(timeout) {
            return Ok(None);
        }
        let mut buf = [0u8; 64];
        match self.file.read(&mut buf) {
            Ok(n) => Ok(Some(buf[..n].to_vec())),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(anyhow!("device read failed: {e} (unplugged?)")),
        }
    }

    pub fn drain(&mut self) {
        while let Ok(Some(_)) = self.read_raw(Duration::ZERO) {}
    }

    fn write_packet(&mut self, pkt: &[u8; PACKET_LEN]) -> Result<()> {
        safety::check(pkt)?;
        self.file
            .write_all(pkt)
            .map_err(|e| anyhow!("device write failed: {e}"))
    }

    /// Send without waiting for a reply.
    pub fn send(&mut self, pkt: [u8; PACKET_LEN]) -> Result<()> {
        self.write_packet(&pkt)
    }

    /// Send and collect replies to the same command id until `done` says stop
    /// or `wait` elapses.
    pub fn request(
        &mut self,
        pkt: [u8; PACKET_LEN],
        wait: Duration,
        mut done: impl FnMut(&[Reply]) -> bool,
    ) -> Result<Vec<Reply>> {
        let cmd = pkt[3];
        self.write_packet(&pkt)?;
        let deadline = Instant::now() + wait;
        let mut out = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let Some(raw) = self.read_raw(left)? else {
                break;
            };
            if raw.len() < 8 || raw[0] != REPORT_IN || raw[1..3] != MAGIC {
                continue;
            }
            if raw[3] == INPUT_REPORT_CMD {
                if let Some(cb) = self.on_input.as_mut() {
                    cb(&raw);
                }
                continue;
            }
            if raw[3] == cmd {
                out.push(Reply(raw[1..].to_vec()));
                if done(&out) {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Checksummed command answered by a single reply.
    pub fn query(&mut self, cmd: u8, payload: &[u8], wait: Duration) -> Result<Option<Reply>> {
        let r = self.request(build(cmd, payload, true), wait, |_| true)?;
        Ok(r.into_iter().next())
    }

    /// Same, but in the raw-length envelope (no checksum) SDL uses.
    pub fn query_raw(&mut self, cmd: u8, payload: &[u8], wait: Duration) -> Result<Option<Reply>> {
        let r = self.request(build(cmd, payload, false), wait, |_| true)?;
        Ok(r.into_iter().next())
    }

    pub fn acked(&mut self, cmd: u8, payload: &[u8]) -> Result<bool> {
        Ok(self
            .query(cmd, payload, Duration::from_millis(500))?
            .is_some())
    }
}

/// Packetised stored-config transfer shared by lighting and mapping profiles.
pub mod blob {
    use super::*;

    pub const PKG: usize = 20;

    pub fn read(dev: &mut Device, cmd: u8, cfg_id: u8, what: &str) -> Result<Vec<u8>> {
        let mut last = String::from("no reply — the pad may be asleep, press a button");
        for _ in 0..3 {
            let mut chunks = std::collections::BTreeMap::new();
            let mut total = 0usize;
            dev.request(
                build(cmd, &[cfg_id, PKG as u8], true),
                Duration::from_millis(1500),
                |r| {
                    let b = &r.last().unwrap().0;
                    total = b[3] as usize;
                    chunks.insert(b[4] as usize, b[6..6 + PKG].to_vec());
                    chunks.len() >= total
                },
            )?;
            if total > 0 && chunks.len() == total {
                return Ok(chunks.into_values().flatten().collect());
            }
            if total > 0 {
                last = format!("got {}/{} packets", chunks.len(), total);
            }
        }
        bail!("reading {what} failed: {last}")
    }

    /// Write only the contiguous run of changed packets. Returns packets sent.
    pub fn write(
        dev: &mut Device,
        start_cmd: u8,
        pack_cmd: u8,
        cfg_id: u8,
        new: &[u8],
        old: Option<&[u8]>,
    ) -> Result<usize> {
        let newp: Vec<&[u8]> = new.chunks(PKG).collect();
        let changed: Vec<usize> = match old {
            Some(old) if old.len() == new.len() => {
                let oldp: Vec<&[u8]> = old.chunks(PKG).collect();
                (0..newp.len()).filter(|&i| newp[i] != oldp[i]).collect()
            }
            _ => (0..newp.len()).collect(),
        };
        let (Some(&first), Some(&last)) = (changed.first(), changed.last()) else {
            return Ok(0);
        };
        let n = last + 1 - first;
        if !dev.acked(start_cmd, &[cfg_id, first as u8, n as u8, PKG as u8])? {
            bail!("pad rejected the write header");
        }
        for (off, i) in (first..=last).enumerate() {
            let mut payload = vec![off as u8];
            payload.extend_from_slice(newp[i]);
            if !dev.acked(pack_cmd, &payload)? {
                bail!("pad rejected packet {i}");
            }
        }
        Ok(n)
    }
}
