//! Background thread that owns the pad. The GUI sends `Cmd`s and receives
//! `Event`s; the worker keeps the last state known to be on the pad and
//! always diffs writes against it.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::bridge::{self, Bridge};
use crate::device::{self, Flags, Info, InputState, Settings};
use crate::evdev;
use crate::hid::Device;
use crate::lang::t;
use crate::lighting::LedConfig;
use crate::profile::{self, Profile};
use crate::screen::{self, Upload};
use crate::tester::Live;

pub enum Cmd {
    Refresh,
    SwitchProfile(u8),
    WriteProfile(Profile),
    SaveProfile,
    WriteLed(LedConfig),
    SaveLed,
    SetFeature(u8, bool),
    SetNumber(u8, u8),
    SetNickname(String),
    Stream(bool),
    /// Factory content in every slot, saved; with `true` the lighting too.
    ResetAll(bool),
    /// Write `profile` (and `led` when given) into `slot` and save it there.
    /// The running slot stays.
    Install {
        profile: Profile,
        led: Option<LedConfig>,
        slot: u8,
    },
    /// Allow apps such as Steam or reWASD to take the pad over.
    SetThirdParty(bool),
    /// Run the grip motors at (left, right) 0..255 for a moment, then stop.
    TestRumble(u8, u8),
    /// Present the pad to games as a DualSense, or stop.
    DualSense(bool),
    /// Share of the games' trigger resistance the pad gets, in %; kept.
    DualSenseResistance(u8),
    /// Send a picture to the screen. Lets go of the pad, which restarts at
    /// the end; ends with `ScreenDone`.
    UploadScreen(Upload),
}

pub enum Event {
    Connected(Info),
    Disconnected(String),
    Settings(Settings),
    Flags(Flags),
    Profile {
        profile: Profile,
        active: u8,
        saved_tag: u16,
    },
    /// A write landed in the pad's working memory (not yet in flash).
    ProfileWritten(Profile),
    Led(LedConfig),
    /// Lighting committed to flash.
    LedSaved,
    Screen(screen::Stage),
    /// The upload is over, and the error if it failed.
    ScreenDone(Result<(), String>),
    Busy(bool),
    Message(String),
    Error(String),
}

pub struct Handle {
    pub tx: Sender<Cmd>,
    pub rx: Receiver<Event>,
    pub input: Arc<Mutex<Live>>,
    pub ds5: Arc<Mutex<bridge::Status>>,
}

pub fn spawn(repaint: impl Fn() + Send + 'static) -> Handle {
    let (tx, cmd_rx) = channel();
    let (ev_tx, rx) = channel();
    let input = Arc::new(Mutex::new(Live::default()));
    let ds5 = Arc::new(Mutex::new(bridge::Status::default()));
    let (shared, ds5_shared) = (input.clone(), ds5.clone());
    thread::spawn(move || {
        Worker {
            ev: Some(ev_tx),
            repaint: Some(Box::new(repaint)),
            input: shared,
            ds5: ds5_shared,
            ..Worker::default()
        }
        .run(cmd_rx)
    });
    Handle { tx, rx, input, ds5 }
}

#[derive(Default)]
struct Worker {
    ev: Option<Sender<Event>>,
    repaint: Option<Box<dyn Fn() + Send>>,
    input: Arc<Mutex<Live>>,
    dev: Option<Device>,
    profile: Option<Profile>,
    led: Option<LedConfig>,
    /// The input tester is reading the pad.
    streaming: bool,
    /// While the tester or the bridge keeps the raw stream on: whether it
    /// was on before they did.
    raw_held: Option<bool>,
    /// DualSense mode.
    bridge: Option<Bridge>,
    ds5: Arc<Mutex<bridge::Status>>,
    /// The gamepad node, read alongside the stream for the tester.
    pad: Option<evdev::Pad>,
    /// Last transport flags read, and when.
    flags: Option<(Flags, Instant)>,
    /// Whether each trigger's rumble bind (82) is on; `None` = unknown,
    /// e.g. left from before this connection.
    bound: [Option<bool>; 2],
}

impl Worker {
    fn send(&self, e: Event) {
        if let Some(tx) = &self.ev {
            let _ = tx.send(e);
        }
        if let Some(r) = &self.repaint {
            r();
        }
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        let mut last_try = Instant::now() - Duration::from_secs(10);
        loop {
            if self.dev.is_none() && last_try.elapsed() > Duration::from_secs(2) {
                last_try = Instant::now();
                self.connect();
            }
            let timeout = if self.streaming || self.bridge.is_some() {
                Duration::from_millis(2)
            } else {
                Duration::from_millis(200)
            };
            match rx.recv_timeout(timeout) {
                Ok(Cmd::UploadScreen(up)) => {
                    self.send(Event::Busy(true));
                    let r = self.upload_screen(up).map_err(|e| format!("{e:#}"));
                    self.send(Event::Busy(false));
                    self.send(Event::ScreenDone(r));
                    last_try = Instant::now();
                }
                // the virtual pad lives on while the real one is away
                Ok(Cmd::DualSense(false)) if self.dev.is_none() => self.stop_bridge(),
                Ok(Cmd::DualSenseResistance(pct)) if self.dev.is_none() => {
                    bridge::Settings { resistance: pct }.store();
                    if let Some(b) = self.bridge.as_mut() {
                        b.set_resistance(pct); // replayed when the pad is back
                    }
                }
                Ok(cmd) => {
                    if self.dev.is_none() {
                        self.send(Event::Error(t("controller not connected").into()));
                        continue;
                    }
                    self.send(Event::Busy(true));
                    let r = self.handle(cmd);
                    self.send(Event::Busy(false));
                    if let Err(e) = r {
                        self.fail(e);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if (self.streaming || self.bridge.is_some()) && self.dev.is_some() {
                self.pump_input();
            }
            if self.bridge.is_some() {
                self.pump_bridge();
            }
            // While apps may take the pad over, keep an eye on who holds it.
            if let Some((f, at)) = &self.flags
                && f.third_party
                && at.elapsed() > Duration::from_secs(3)
                && self.dev.is_some()
                && let Err(e) = self.read_flags()
            {
                self.fail(e);
            }
        }
        // App closing: the virtual pad goes, and the stream if we started it.
        self.stop_bridge();
        self.streaming = false;
        let _ = self.release_stream();
    }

    fn fail(&mut self, e: anyhow::Error) {
        let msg = format!("{e:#}");
        if msg.contains("device read failed") || msg.contains("device write failed") {
            self.dev = None;
            self.streaming = false;
            self.raw_held = None;
            self.pad = None;
            self.flags = None;
            if let Some(b) = self.bridge.as_mut() {
                b.detach();
            }
            self.input.lock().unwrap().stop();
            self.send(Event::Disconnected(msg));
        } else {
            self.send(Event::Error(msg));
        }
    }

    fn connect(&mut self) {
        match Device::open() {
            Ok(mut d) => match device::read_info(&mut d) {
                Ok(info) if info.supported() => {
                    self.dev = Some(d);
                    self.bound = [None; 2];
                    self.send(Event::Connected(info));
                    if let Err(e) = self.refresh().and_then(|_| self.reattach_bridge()) {
                        self.fail(e);
                    }
                }
                Ok(info) => self.send(Event::Disconnected(crate::tr!(
                    "unsupported device connected (type {kind})",
                    kind = info.device_type
                ))),
                Err(e) => self.send(Event::Disconnected(format!("{e:#}"))),
            },
            Err(e) => self.send(Event::Disconnected(format!("{e:#}"))),
        }
    }

    fn dev(&mut self) -> &mut Device {
        self.dev.as_mut().expect("connected")
    }

    /// Reads the transport flags and tells the app when they changed.
    fn read_flags(&mut self) -> Result<()> {
        let f = device::read_flags(self.dev())?;
        let changed = self.flags.as_ref().is_none_or(|(old, _)| {
            (old.third_party, &old.control_by) != (f.third_party, &f.control_by)
        });
        self.flags = Some((f.clone(), Instant::now()));
        if changed {
            self.send(Event::Flags(f));
        }
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        let s = device::read_settings(self.dev())?;
        self.send(Event::Settings(s));
        self.flags = None;
        self.read_flags()?;
        let st = profile::status(self.dev())?;
        self.load_profile(st.active, st.active, st.versions[st.active as usize])
    }

    /// Read `slot` and its lighting (either read makes it active) and
    /// re-engage its trigger effects.
    fn load_profile(&mut self, slot: u8, active: u8, tag: u16) -> Result<()> {
        let led = LedConfig::read(self.dev(), slot)?;
        self.led = Some(led.clone());
        self.send(Event::Led(led));
        let p = profile::read(self.dev(), slot)?;
        self.profile = Some(p.clone());
        self.engage(&p, false)?;
        self.send(Event::Profile {
            profile: p,
            active,
            saved_tag: tag,
        });
        Ok(())
    }

    /// Puts `make(slot's profile)` and, when given, `led` into `slot` and
    /// saves both to flash. Leaves `slot` running. Returns whether anything
    /// had to be written.
    fn put(
        &mut self,
        slot: u8,
        make: impl FnOnce(&Profile) -> Profile,
        led: Option<LedConfig>,
    ) -> Result<bool> {
        // either read makes `slot` the running one, which writes need
        let old = profile::read(self.dev(), slot)?;
        let new = make(&old);
        let old_led = LedConfig::read(self.dev(), slot)?;
        let led = led
            .map(|l| LedConfig { slot, ..l })
            .filter(|l| *l != old_led);
        if new.blob == old.blob && led.is_none() {
            return Ok(false); // spare the flash
        }
        profile::write(self.dev(), &new, &old)?;
        if let Some(l) = led {
            l.write(self.dev(), Some(&old_led))?;
        }
        let st = profile::status(self.dev())?;
        let tag = profile::next_tag(st.versions[slot as usize]);
        profile::save(self.dev(), slot, tag)?;
        Ok(true)
    }

    /// Runs `f`, which may switch slots, then makes the slot that was
    /// running before the running one again and reloads it.
    fn keeping_slot<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let back = profile::status(self.dev())?.active;
        let r = f(self);
        let st = profile::status(self.dev())?;
        self.load_profile(back, back, st.versions[back as usize])?;
        // an effect engaged from before may be gone from the profile now
        if let Some(p) = self.profile.clone() {
            self.engage(&p, true)?;
        }
        r
    }

    /// Live-send stored trigger effects as Space Station does: 81 for modes
    /// 0-4, the 82 rumble bind for Vibration. A side that is not Vibration
    /// has its bind released unless it is known to be free, since a bind
    /// outlives profile switches. With `clear` a side stored as Normal is
    /// actively cleared, and one going to Vibration loses its old effect.
    fn engage(&mut self, p: &Profile, clear: bool) -> Result<()> {
        if self.bridge.is_some() {
            return Ok(()); // in DualSense mode the game drives the triggers
        }
        for (side, id) in [(0, device::SIDE_LEFT), (1, device::SIDE_RIGHT)] {
            let f = p.force(side);
            let vibration = f.mode == profile::MODE_VIBRATION;
            if !vibration && self.bound[side] != Some(false) {
                device::bind_trigger(self.dev(), id, &f.bind, false)?;
                self.bound[side] = Some(false);
            }
            match f.mode {
                1..=4 => device::engage_trigger(self.dev(), id, f.mode, &f.params)?,
                0 if clear => device::engage_trigger(self.dev(), id, 0, &f.params)?,
                _ if vibration => {
                    if clear {
                        device::engage_trigger(self.dev(), id, 0, &f.params)?;
                    }
                    device::bind_trigger(self.dev(), id, &f.bind, true)?;
                    self.bound[side] = Some(true);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle(&mut self, cmd: Cmd) -> Result<()> {
        match cmd {
            Cmd::Refresh => self.refresh()?,
            Cmd::SwitchProfile(slot) => {
                let st = profile::status(self.dev())?;
                self.load_profile(slot, slot, st.versions[slot as usize])?;
                self.send(Event::Message(crate::tr!(
                    "Profile {slot} active",
                    slot = slot + 1
                )));
            }
            Cmd::WriteProfile(new) => {
                let old = self.profile.clone().filter(|o| o.slot == new.slot);
                let Some(old) = old else {
                    anyhow::bail!(t("the profile on the controller changed — refresh"))
                };
                let st = profile::status(self.dev())?;
                if st.active != new.slot {
                    anyhow::bail!(crate::tr!(
                        "the controller switched to profile {active}, changes not written",
                        active = st.active + 1
                    ));
                }
                let n = profile::write(self.dev(), &new, &old)?;
                let force_changed = (0..2).any(|s| new.force(s) != old.force(s));
                self.profile = Some(new.clone());
                if force_changed || new.macro_bytes_differ(&old) {
                    self.engage(&new, true)?;
                }
                self.send(Event::ProfileWritten(new));
                self.send(Event::Message(crate::tr!(
                    "Written to the controller ({n} packets). Not saved to flash.",
                    n
                )));
            }
            Cmd::SaveProfile => {
                let Some(mut p) = self.profile.clone() else {
                    anyhow::bail!(t("profile not loaded"))
                };
                let st = profile::status(self.dev())?;
                let tag = profile::next_tag(st.versions[p.slot as usize]);
                profile::save(self.dev(), p.slot, tag)?;
                p.blob[225..227].copy_from_slice(&tag.to_le_bytes());
                self.profile = Some(p.clone());
                self.send(Event::Profile {
                    profile: p,
                    active: st.active,
                    saved_tag: tag,
                });
                self.send(Event::Message(
                    t("Profile saved to the controller's flash").into(),
                ));
            }
            Cmd::WriteLed(new) => {
                let st = profile::status(self.dev())?;
                if st.active != new.slot {
                    anyhow::bail!(crate::tr!(
                        "the controller switched to profile {active}, lighting not written",
                        active = st.active + 1
                    ));
                }
                let old = self.led.clone();
                new.write(self.dev(), old.as_ref())?;
                self.led = Some(new);
            }
            Cmd::SaveLed => {
                // Lighting has no tag of its own: keep the active profile's.
                let st = profile::status(self.dev())?;
                profile::save(self.dev(), st.active, st.versions[st.active as usize])?;
                self.send(Event::LedSaved);
                self.send(Event::Message(
                    t("Lighting saved to the controller's memory").into(),
                ));
            }
            Cmd::ResetAll(lighting) => {
                let n = self.keeping_slot(|w| {
                    let mut n = 0;
                    for slot in 0..profile::SLOTS {
                        let led = lighting.then(|| LedConfig::factory(slot));
                        let reset = |old: &Profile| {
                            let mut p = old.clone();
                            p.factory_reset();
                            p
                        };
                        n += w.put(slot, reset, led)? as usize;
                    }
                    Ok(n)
                })?;
                self.send(Event::Message(if n == 0 {
                    t("All profiles are already factory").into()
                } else {
                    crate::tr!("Profiles reset: {n}, saved to flash", n)
                }));
            }
            Cmd::Install {
                profile: p,
                led,
                slot,
            } => {
                let changed = self.keeping_slot(|w| w.put(slot, |old| p.onto(old), led))?;
                self.send(Event::Message(if changed {
                    crate::tr!(
                        "Written to profile {slot} and saved to flash",
                        slot = slot + 1
                    )
                } else {
                    crate::tr!("Profile {slot} is already like this", slot = slot + 1)
                }));
            }
            Cmd::SetThirdParty(on) => {
                device::set_third_party(self.dev(), on)?;
                self.read_flags()?;
            }
            Cmd::SetFeature(sub, on) => {
                device::set_feature(self.dev(), sub, on)?;
                let s = device::read_settings(self.dev())?;
                self.send(Event::Settings(s));
            }
            Cmd::SetNumber(c, v) => {
                device::set_number(self.dev(), c, v)?;
                let s = device::read_settings(self.dev())?;
                self.send(Event::Settings(s));
            }
            Cmd::SetNickname(n) => {
                device::write_nickname(self.dev(), &n)?;
                let info = device::read_info(self.dev())?;
                self.send(Event::Connected(info));
                self.send(Event::Message(t("Name written").into()));
            }
            Cmd::TestRumble(left, right) => {
                let run = device::rumble(self.dev(), left, right);
                let until = Instant::now() + Duration::from_millis(800);
                while run.is_ok() && Instant::now() < until {
                    // keep the input tester fed while the motors run
                    if self.streaming {
                        self.pump_input();
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                // Always stop: the motors keep running until told otherwise.
                let stop = match self.dev.as_mut() {
                    Some(d) => device::rumble(d, 0, 0),
                    None => Ok(()), // gone, and its motors with it
                };
                run.and(stop)?;
            }
            Cmd::UploadScreen(_) => unreachable!("run() takes it"),
            Cmd::DualSense(true) if self.bridge.is_none() => {
                self.bridge = Some(Bridge::start(self.ds5.clone())?);
                if let Err(e) = self.reattach_bridge() {
                    self.stop_bridge();
                    return Err(e);
                }
                self.send(Event::Message(t("DualSense mode on").into()));
            }
            Cmd::DualSense(true) => {}
            Cmd::DualSenseResistance(pct) => {
                bridge::Settings { resistance: pct }.store();
                if let Some(b) = self.bridge.as_mut() {
                    let now = b.set_resistance(pct);
                    self.act(now)?;
                }
            }
            Cmd::DualSense(false) => {
                self.stop_bridge();
                self.send(Event::Message(t("DualSense mode off").into()));
            }
            Cmd::Stream(on) => {
                if on && !self.streaming {
                    self.hold_stream()?;
                    self.streaming = true;
                    self.install_input_hook();
                    // Best effort: without it the tester shows the sensor only.
                    self.pad = evdev::find(&self.dev().path)
                        .and_then(|p| evdev::Pad::open(&p))
                        .ok();
                } else if !on && self.streaming {
                    self.streaming = false;
                    self.pad = None;
                    self.input.lock().unwrap().stop();
                    self.release_stream()?;
                }
            }
        }
        Ok(())
    }

    /// Checks what can be checked while connected, then hands the pad over
    /// to `screen::upload` and waits out the screen's save.
    fn upload_screen(&mut self, up: Upload) -> Result<()> {
        let retry = screen::find_port().is_some();
        if !retry {
            let Some(d) = self.dev.as_mut() else {
                anyhow::bail!(t("controller not connected"))
            };
            if !screen::rule_installed() {
                anyhow::bail!(screen::rule_hint());
            }
            if !device::read_info(d)?.wired {
                anyhow::bail!(t("a picture can be uploaded by cable only"));
            }
            if self.raw_held == Some(false) {
                device::set_stream(d, false)?;
            }
        }
        self.streaming = false;
        self.raw_held = None;
        if let Some(b) = self.bridge.as_mut() {
            b.detach();
        }
        self.pad = None;
        self.flags = None;
        self.input.lock().unwrap().stop();
        let dev = self.dev.take();
        self.send(Event::Disconnected(
            t("Uploading the picture to the screen").into(),
        ));
        screen::upload(dev, &up, &mut |s| self.send(Event::Screen(s)))?;
        // the screen saves and the pad restarts; do not talk to it meanwhile
        thread::sleep(Duration::from_secs(20));
        Ok(())
    }

    fn install_input_hook(&mut self) {
        let shared = self.input.clone();
        self.dev().on_input = Some(Box::new(move |raw| {
            if let Some(s) = InputState::parse(raw) {
                shared.lock().unwrap().record(s);
            }
        }));
    }

    /// Vendor reports go to the tester and, for motion, to the bridge; the
    /// gamepad node goes to the tester.
    fn pump_input(&mut self) {
        let mut got = false;
        loop {
            let r = self.dev().read_raw(Duration::ZERO);
            match r {
                Ok(Some(raw)) => {
                    if let Some(s) = InputState::parse(&raw) {
                        if self.streaming {
                            self.input.lock().unwrap().record(s);
                            got = true;
                        }
                        if let Some(b) = self.bridge.as_mut() {
                            b.motion(&s);
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    self.fail(e);
                    return;
                }
            }
        }
        if let Some(pad) = self.pad.as_mut() {
            match pad.read() {
                Ok(reports) => {
                    let mut live = self.input.lock().unwrap();
                    for o in reports {
                        live.record_output(o);
                        got = true;
                    }
                }
                Err(_) => self.pad = None,
            }
        }
        if got && let Some(r) = &self.repaint {
            r();
        }
    }

    /// Keep the raw stream on for the tester or the bridge.
    fn hold_stream(&mut self) -> Result<()> {
        if self.raw_held.is_none() {
            let was = device::read_flags(self.dev())?.raw_data;
            if !was {
                device::set_stream(self.dev(), true)?;
            }
            self.raw_held = Some(was);
        }
        Ok(())
    }

    /// Turn the raw stream back off once neither needs it, if we turned it on.
    fn release_stream(&mut self) -> Result<()> {
        if self.streaming || self.bridge.is_some() {
            return Ok(());
        }
        if self.raw_held.take() == Some(false)
            && let Some(d) = self.dev.as_mut()
        {
            device::set_stream(d, false)?;
        }
        Ok(())
    }

    /// The real pad is (back) here: feed the virtual one from it and give it
    /// the effects the game set meanwhile.
    fn reattach_bridge(&mut self) -> Result<()> {
        if self.bridge.is_none() {
            return Ok(());
        }
        self.hold_stream()?;
        let path = self.dev().path.clone();
        let b = self.bridge.as_mut().expect("checked");
        b.attach(&path);
        let replay = b.replay();
        self.act(replay)
    }

    fn pump_bridge(&mut self) {
        let rumble = self
            .profile
            .as_ref()
            .is_none_or(|p| p.grip_vibration().master);
        let Some(b) = self.bridge.as_mut() else {
            return;
        };
        let actions = match b.pump(rumble) {
            Ok(a) => a,
            Err(e) => {
                self.stop_bridge();
                self.send(Event::Error(crate::tr!(
                    "DualSense mode stopped: {e}",
                    e = format!("{e:#}")
                )));
                return;
            }
        };
        if self.dev.is_some()
            && let Err(e) = self.act(actions)
        {
            self.fail(e);
        }
    }

    /// Send the game's effects to the pad, without waiting for answers: the
    /// game can change them dozens of times a second.
    fn act(&mut self, actions: Vec<bridge::Action>) -> Result<()> {
        for a in actions {
            match a {
                bridge::Action::Trigger(side, e) => {
                    let id = [device::SIDE_LEFT, device::SIDE_RIGHT][side];
                    self.dev()
                        .send(device::trigger_packet(id, e.mode, &e.params))?;
                }
                bridge::Action::Rumble(l, r) => device::rumble(self.dev(), l, r)?,
            }
        }
        Ok(())
    }

    /// Remove the virtual pad and give the triggers back to the profile.
    fn stop_bridge(&mut self) {
        if self.bridge.take().is_none() {
            return;
        }
        if self.dev.is_none() {
            return;
        }
        let r = device::rumble(self.dev(), 0, 0)
            .and_then(|_| self.release_stream())
            .and_then(|_| match self.profile.clone() {
                Some(p) => self.engage(&p, true),
                None => Ok(()),
            });
        if let Err(e) = r {
            self.fail(e);
        }
    }
}
