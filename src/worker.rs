//! Background thread that owns the pad. The GUI sends `Cmd`s and receives
//! `Event`s; the worker keeps the last state known to be on the pad and
//! always diffs writes against it.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::device::{self, Flags, Info, InputState, Settings};
use crate::evdev;
use crate::hid::Device;
use crate::lighting::LedConfig;
use crate::profile::{self, Profile};
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
    /// Grip vibration bytes stay as they are.
    ResetAll(bool),
    /// Write `profile` (and `led` when given) into `slot` and save it there,
    /// keeping the slot's vibration settings. The running slot stays.
    Install {
        profile: Profile,
        led: Option<LedConfig>,
        slot: u8,
    },
    /// Allow apps such as Steam or reWASD to take the pad over.
    SetThirdParty(bool),
    /// Run the grip motors at (left, right) 0..255 for a moment, then stop.
    TestRumble(u8, u8),
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
    Busy(bool),
    Message(String),
    Error(String),
}

pub struct Handle {
    pub tx: Sender<Cmd>,
    pub rx: Receiver<Event>,
    pub input: Arc<Mutex<Live>>,
}

pub fn spawn(repaint: impl Fn() + Send + 'static) -> Handle {
    let (tx, cmd_rx) = channel();
    let (ev_tx, rx) = channel();
    let input = Arc::new(Mutex::new(Live::default()));
    let shared = input.clone();
    thread::spawn(move || {
        Worker {
            ev: Some(ev_tx),
            repaint: Some(Box::new(repaint)),
            input: shared,
            ..Worker::default()
        }
        .run(cmd_rx)
    });
    Handle { tx, rx, input }
}

#[derive(Default)]
struct Worker {
    ev: Option<Sender<Event>>,
    repaint: Option<Box<dyn Fn() + Send>>,
    input: Arc<Mutex<Live>>,
    dev: Option<Device>,
    profile: Option<Profile>,
    led: Option<LedConfig>,
    streaming: bool,
    stream_was_on: bool,
    /// The gamepad node, read alongside the stream for the tester.
    pad: Option<evdev::Pad>,
    /// Last transport flags read, and when.
    flags: Option<(Flags, Instant)>,
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
            let timeout = if self.streaming {
                Duration::from_millis(2)
            } else {
                Duration::from_millis(200)
            };
            match rx.recv_timeout(timeout) {
                Ok(cmd) => {
                    if self.dev.is_none() {
                        self.send(Event::Error("геймпад не подключён".into()));
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
            if self.streaming {
                self.pump_input();
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
        // App closing: stop the stream if we started it.
        if self.streaming
            && !self.stream_was_on
            && let Some(d) = self.dev.as_mut()
        {
            let _ = device::set_stream(d, false);
        }
    }

    fn fail(&mut self, e: anyhow::Error) {
        let msg = format!("{e:#}");
        if msg.contains("device read failed") || msg.contains("device write failed") {
            self.dev = None;
            self.streaming = false;
            self.pad = None;
            self.flags = None;
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
                    self.send(Event::Connected(info));
                    if let Err(e) = self.refresh() {
                        self.fail(e);
                    }
                }
                Ok(info) => self.send(Event::Disconnected(format!(
                    "подключено неподдерживаемое устройство (тип {})",
                    info.device_type
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
    /// re-engage its resistance effects.
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
        // a resistance engaged from before may be gone from the profile now
        if let Some(p) = self.profile.clone() {
            self.engage(&p, true)?;
        }
        r
    }

    /// Live-send stored trigger effects. Only resistance modes are ever sent;
    /// with `clear` a side stored as Normal is actively cleared.
    fn engage(&mut self, p: &Profile, clear: bool) -> Result<()> {
        for (side, id) in [(0, device::SIDE_LEFT), (1, device::SIDE_RIGHT)] {
            let f = p.force(side);
            match f.mode {
                1 | 3 | 4 => device::engage_trigger(self.dev(), id, f.mode, &f.params)?,
                0 if clear => device::engage_trigger(self.dev(), id, 0, &f.params)?,
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
                self.send(Event::Message(format!("Активен профиль {}", slot + 1)));
            }
            Cmd::WriteProfile(new) => {
                let old = self.profile.clone().filter(|o| o.slot == new.slot);
                let Some(old) = old else {
                    anyhow::bail!("профиль на геймпаде сменился — обновите")
                };
                let st = profile::status(self.dev())?;
                if st.active != new.slot {
                    anyhow::bail!(
                        "на геймпаде переключён профиль ({}), изменения не записаны",
                        st.active + 1
                    );
                }
                let n = profile::write(self.dev(), &new, &old)?;
                let force_changed = (0..2).any(|s| new.force(s) != old.force(s));
                self.profile = Some(new.clone());
                if force_changed || new.macro_bytes_differ(&old) {
                    self.engage(&new, true)?;
                }
                self.send(Event::ProfileWritten(new));
                self.send(Event::Message(format!(
                    "Записано в геймпад ({n} пакетов). Не сохранено во флеш."
                )));
            }
            Cmd::SaveProfile => {
                let Some(mut p) = self.profile.clone() else {
                    anyhow::bail!("профиль не загружен")
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
                    "Профиль сохранён во флеш-память геймпада".into(),
                ));
            }
            Cmd::WriteLed(new) => {
                let st = profile::status(self.dev())?;
                if st.active != new.slot {
                    anyhow::bail!(
                        "на геймпаде переключён профиль ({}), подсветка не записана",
                        st.active + 1
                    );
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
                    "Подсветка сохранена в памяти геймпада".into(),
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
                    "Все профили уже заводские".into()
                } else {
                    format!("Сброшено профилей: {n}, сохранено во флеш")
                }));
            }
            Cmd::Install {
                profile: p,
                led,
                slot,
            } => {
                let changed = self.keeping_slot(|w| w.put(slot, |old| p.onto(old), led))?;
                self.send(Event::Message(if changed {
                    format!("Записано в профиль {} и сохранено во флеш", slot + 1)
                } else {
                    format!("Профиль {} уже такой", slot + 1)
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
                self.send(Event::Message("Имя записано".into()));
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
            Cmd::Stream(on) => {
                if on && !self.streaming {
                    self.stream_was_on = device::read_flags(self.dev())?.raw_data;
                    if !self.stream_was_on {
                        device::set_stream(self.dev(), true)?;
                    }
                    self.streaming = true;
                    self.install_input_hook();
                    // Best effort: without it the tester shows the sensor only.
                    self.pad = evdev::find(&self.dev().path)
                        .and_then(|p| evdev::Pad::open(&p))
                        .ok();
                } else if !on && self.streaming {
                    self.streaming = false;
                    self.pad = None;
                    if !self.stream_was_on {
                        device::set_stream(self.dev(), false)?;
                    }
                    self.input.lock().unwrap().stop();
                }
            }
        }
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

    fn pump_input(&mut self) {
        let mut got = false;
        loop {
            let r = self.dev().read_raw(Duration::ZERO);
            match r {
                Ok(Some(raw)) => {
                    if let Some(s) = InputState::parse(&raw) {
                        self.input.lock().unwrap().record(s);
                        got = true;
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
}
