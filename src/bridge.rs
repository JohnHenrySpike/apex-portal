//! DualSense mode: a virtual DualSense fed from the Apex 5, so games that
//! speak DualSense send their trigger effects, which go back to the Apex.
//!
//!   Apex 5 ──xpad + motion──► Bridge ──► virtual DualSense (uhid) ──► game
//!      ▲                                                               │
//!      └──── command 81 / 0x12 ◄──── trigger effects, rumble ◄────────┘
//!
//! The virtual pad outlives the real one: when the Apex sleeps or is
//! unplugged the game keeps its controller, with nothing pressed, and gets
//! its effects back when the Apex returns.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::device::InputState;
use crate::ds5::{self, Apex, Effect};
use crate::{evdev, uhid};

/// What the app shows about the bridge.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub on: bool,
    /// Something holds the virtual pad open (Steam, a game).
    pub opened: bool,
    /// The real pad's input reaches the virtual one.
    pub pad: bool,
    /// Left and right: the game's last effect and what the Apex got for it.
    pub effects: [Option<(Effect, Option<Apex>)>; 2],
}

/// Games keep a rumble alive by sending it again (SDL every 2 s); after
/// this long without a word from the game, nothing should still shake.
/// No Man's Sky leaves a small rumble set when it stops sending.
const QUIET: Duration = Duration::from_secs(3);

/// What the worker has to send to the Apex.
#[derive(Debug, PartialEq)]
pub enum Action {
    Trigger(usize, Apex),
    Rumble(u8, u8),
}

pub struct Bridge {
    dev: uhid::Device,
    input: ds5::Input,
    pad: Option<evdev::Pad>,
    seq: u8,
    start: Instant,
    dirty: bool,
    /// Last effect sent per trigger, sent again when the Apex comes back.
    effects: [Option<Apex>; 2],
    /// The game's last effect per trigger, to skip repeats.
    game: [Option<Effect>; 2],
    rumble: Option<(u8, u8)>,
    /// When the game last sent anything, and whether shaking was stopped
    /// since because it went quiet.
    heard: Instant,
    hushed: bool,
    /// Output reports, one per line, when APEX5_DS5_LOG names a file.
    log: Option<File>,
    status: Arc<Mutex<Status>>,
}

impl Bridge {
    pub fn start(status: Arc<Mutex<Status>>) -> Result<Bridge> {
        let dev = uhid::Device::create(
            &uhid::Identity {
                name: ds5::NAME,
                bus: uhid::BUS_USB,
                vendor: ds5::VENDOR,
                product: ds5::PRODUCT,
                version: ds5::VERSION,
                descriptor: &ds5::descriptor(),
            },
            ds5::features(),
        )?;
        *status.lock().unwrap() = Status {
            on: true,
            ..Status::default()
        };
        let mut b = Bridge {
            dev,
            input: ds5::Input::default(),
            pad: None,
            seq: 0,
            start: Instant::now(),
            dirty: true,
            effects: [None; 2],
            game: [None; 2],
            rumble: None,
            heard: Instant::now(),
            hushed: true,
            log: std::env::var_os("APEX5_DS5_LOG").and_then(|p| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .ok()
            }),
            status,
        };
        b.send_input()?;
        Ok(b)
    }

    /// Reads the real pad's gamepad node, the one next to `hidraw`.
    pub fn attach(&mut self, hidraw: &Path) {
        self.pad = evdev::find(hidraw).and_then(|p| evdev::Pad::open(&p)).ok();
        if let Some(p) = &self.pad {
            self.input.set_pad(&p.output());
        }
        self.dirty = true;
        self.status.lock().unwrap().pad = self.pad.is_some();
    }

    /// The real pad went away: let go of everything it held.
    pub fn detach(&mut self) {
        self.pad = None;
        self.input.release();
        self.rumble = None;
        self.dirty = true;
        self.status.lock().unwrap().pad = false;
    }

    /// The effects the game set, for an Apex that has just come back.
    pub fn replay(&self) -> Vec<Action> {
        (0..2)
            .filter_map(|side| self.effects[side].map(|a| Action::Trigger(side, a)))
            .collect()
    }

    pub fn motion(&mut self, s: &InputState) {
        self.input.set_motion(s.gyro, s.accel);
        self.dirty = true;
    }

    /// Moves input to the virtual pad and returns what the game asked of
    /// the real one. `rumble` says whether the profile lets the grips shake.
    pub fn pump(&mut self, rumble: bool) -> Result<Vec<Action>> {
        if let Some(p) = self.pad.as_mut() {
            match p.read() {
                Ok(reports) => {
                    if let Some(o) = reports.last() {
                        self.input.set_pad(o);
                        self.dirty = true;
                    }
                }
                Err(_) => self.detach(),
            }
        }
        let mut out = Vec::new();
        let was_open = self.dev.opened;
        for uhid::Event::Output(data) in self.dev.poll()? {
            if let Some(f) = self.log.as_mut() {
                let t = self.start.elapsed().as_secs_f64();
                let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
                let _ = writeln!(f, "{t:.3} {hex}");
            }
            let c = ds5::parse_output(&data);
            // Steam's own reports (lightbar, player LEDs) keep coming
            // through menus; only the game's rumble and effects count
            if c.rumble.is_some() || c.triggers.iter().any(Option::is_some) {
                self.heard = Instant::now();
                self.hushed = false;
            }
            if let Some(r) = c.rumble {
                let r = if rumble { r } else { (0, 0) };
                if self.rumble != Some(r) {
                    self.rumble = Some(r);
                    out.push(Action::Rumble(r.0, r.1));
                }
            }
            for (side, e) in c.triggers.iter().enumerate() {
                let Some(e) = e else { continue };
                if self.game[side] == Some(*e) {
                    continue;
                }
                self.game[side] = Some(*e);
                let a = ds5::translate(e);
                self.status.lock().unwrap().effects[side] = Some((*e, a));
                if let Some(a) = a
                    && self.effects[side] != Some(a)
                {
                    self.effects[side] = Some(a);
                    out.push(Action::Trigger(side, a));
                }
            }
        }
        if was_open && !self.dev.opened {
            // Steam or the game let go: nothing of theirs may stay on
            out.extend(self.stop_effects(true));
        } else if !self.hushed && self.heard.elapsed() > QUIET {
            out.extend(self.stop_effects(false));
            self.hushed = true;
        }
        self.status.lock().unwrap().opened = self.dev.opened;
        if self.dirty {
            self.send_input()?;
        }
        Ok(out)
    }

    /// Stops the grips and every shaking trigger; with `all`, every trigger
    /// effect. A repeat of the same effect from the game then counts again.
    fn stop_effects(&mut self, all: bool) -> Vec<Action> {
        let mut out = Vec::new();
        if self.rumble.is_some_and(|r| r != (0, 0)) {
            self.rumble = Some((0, 0));
            out.push(Action::Rumble(0, 0));
        }
        let mut st = self.status.lock().unwrap();
        for side in 0..2 {
            let Some(a) = self.effects[side] else {
                continue;
            };
            if a != ds5::OFF && (all || a.mode == 2) {
                self.effects[side] = Some(ds5::OFF);
                self.game[side] = None;
                if let Some((e, _)) = st.effects[side] {
                    st.effects[side] = Some((e, Some(ds5::OFF)));
                }
                out.push(Action::Trigger(side, ds5::OFF));
            }
        }
        out
    }

    fn send_input(&mut self) -> Result<()> {
        // the DualSense counts sensor time in thirds of a microsecond
        let t = (self.start.elapsed().as_micros() * 3) as u32;
        self.dev.input(&self.input.report(self.seq, t))?;
        self.seq = self.seq.wrapping_add(1);
        self.dirty = false;
        Ok(())
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        *self.status.lock().unwrap() = Status::default();
    }
}
