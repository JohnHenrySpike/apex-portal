//! A HID device made in userspace through /dev/uhid: the kernel binds a
//! driver to it as to real hardware, and output reports a program writes to
//! it come back here. Layout from linux/uapi/linux/uhid.h (packed structs).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result, anyhow, bail};

pub const PATH: &str = "/dev/uhid";
pub const BUS_USB: u16 = 0x03;

const DESTROY: u32 = 1;
const START: u32 = 2;
const STOP: u32 = 3;
const OPEN: u32 = 4;
const CLOSE: u32 = 5;
const OUTPUT: u32 = 6;
const GET_REPORT: u32 = 9;
const GET_REPORT_REPLY: u32 = 10;
const CREATE2: u32 = 11;
const INPUT2: u32 = 12;
const SET_REPORT: u32 = 13;
const SET_REPORT_REPLY: u32 = 14;

const DATA_MAX: usize = 4096;
/// `struct uhid_event`: a u32 type and the largest union member, create2.
const EVENT_LEN: usize = 4 + 128 + 64 + 64 + 2 + 2 + 4 * 4 + DATA_MAX;
const EIO: u16 = 5;

/// How the device introduces itself to the kernel.
pub struct Identity<'a> {
    pub name: &'a str,
    pub bus: u16,
    pub vendor: u32,
    pub product: u32,
    pub version: u32,
    pub descriptor: &'a [u8],
}

pub struct Device {
    file: File,
    /// Feature reports by number, with their report id in front.
    features: Vec<(u8, Vec<u8>)>,
    /// Something (Steam, a game, hid-playstation's input nodes) has it open.
    pub opened: bool,
}

/// What came back from the kernel side.
pub enum Event {
    Output(Vec<u8>),
}

impl Device {
    pub fn create(id: &Identity, features: Vec<(u8, Vec<u8>)>) -> Result<Device> {
        if id.descriptor.len() > DATA_MAX {
            bail!("report descriptor too large");
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(PATH)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => anyhow!("нет {PATH}: модуль uhid не загружен"),
                std::io::ErrorKind::PermissionDenied => {
                    anyhow!("нет доступа к {PATH}: нужно правило udev")
                }
                _ => anyhow!("{PATH}: {e}"),
            })?;
        let mut ev = event(CREATE2);
        put_str(&mut ev[4..132], id.name);
        let o = 4 + 128 + 64 + 64;
        ev[o..o + 2].copy_from_slice(&(id.descriptor.len() as u16).to_ne_bytes());
        ev[o + 2..o + 4].copy_from_slice(&id.bus.to_ne_bytes());
        ev[o + 4..o + 8].copy_from_slice(&id.vendor.to_ne_bytes());
        ev[o + 8..o + 12].copy_from_slice(&id.product.to_ne_bytes());
        ev[o + 12..o + 16].copy_from_slice(&id.version.to_ne_bytes());
        // country stays 0
        ev[o + 20..o + 20 + id.descriptor.len()].copy_from_slice(id.descriptor);
        let mut dev = Device {
            file,
            features,
            opened: false,
        };
        dev.write(&ev).context("create the virtual device")?;
        Ok(dev)
    }

    fn write(&mut self, ev: &[u8]) -> Result<()> {
        self.file
            .write_all(ev)
            .map_err(|e| anyhow!("{PATH} write failed: {e}"))
    }

    /// Hand an input report (with its report id) to the kernel.
    pub fn input(&mut self, report: &[u8]) -> Result<()> {
        let mut ev = event(INPUT2);
        ev[4..6].copy_from_slice(&(report.len() as u16).to_ne_bytes());
        ev[6..6 + report.len()].copy_from_slice(report);
        self.write(&ev)
    }

    /// Everything queued: output reports are returned, report requests are
    /// answered here so readers never hang waiting on us.
    pub fn poll(&mut self) -> Result<Vec<Event>> {
        let mut out = Vec::new();
        let mut ev = vec![0u8; EVENT_LEN];
        loop {
            let mut pfd = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd, zero timeout.
            if unsafe { libc::poll(&mut pfd, 1, 0) } <= 0 {
                break;
            }
            let n = match self.file.read(&mut ev) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => bail!("{PATH} read failed: {e}"),
            };
            if n < 4 {
                break;
            }
            let u32_at = |o: usize| u32::from_ne_bytes(ev[o..o + 4].try_into().unwrap());
            let u16_at = |o: usize| u16::from_ne_bytes([ev[o], ev[o + 1]]) as usize;
            match u32_at(0) {
                OPEN => self.opened = true,
                CLOSE => self.opened = false,
                START | STOP => {}
                OUTPUT => {
                    let size = u16_at(4 + DATA_MAX).min(DATA_MAX);
                    out.push(Event::Output(ev[4..4 + size].to_vec()));
                }
                GET_REPORT => {
                    let (id, rnum) = (u32_at(4), ev[8]);
                    let mut r = event(GET_REPORT_REPLY);
                    r[4..8].copy_from_slice(&id.to_ne_bytes());
                    match self.features.iter().find(|(n, _)| *n == rnum) {
                        Some((_, body)) => {
                            r[10..12].copy_from_slice(&(body.len() as u16).to_ne_bytes());
                            r[12..12 + body.len()].copy_from_slice(body);
                        }
                        None => r[8..10].copy_from_slice(&EIO.to_ne_bytes()),
                    }
                    self.write(&r)?;
                }
                SET_REPORT => {
                    let id = u32_at(4);
                    let mut r = event(SET_REPORT_REPLY);
                    r[4..8].copy_from_slice(&id.to_ne_bytes());
                    self.write(&r)?;
                    let size = u16_at(10).min(DATA_MAX);
                    out.push(Event::Output(ev[12..12 + size].to_vec()));
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.write(&event(DESTROY));
    }
}

fn event(kind: u32) -> Vec<u8> {
    let mut ev = vec![0u8; EVENT_LEN];
    ev[..4].copy_from_slice(&kind.to_ne_bytes());
    ev
}

fn put_str(dst: &mut [u8], s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&b[..n]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_is_the_kernels_size() {
        // sizeof(struct uhid_event) on Linux: 4 + sizeof(struct uhid_create2_req)
        assert_eq!(EVENT_LEN, 4376);
    }
}
