//! Apex 5 Control — native configuration app for the Flydigi Apex 5.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use apex5::device::{Flags, Info, Settings};
use apex5::evdev::Output;
use apex5::lang::t;
use apex5::lighting::{Effect, LedConfig, Rgb};
use apex5::profile::{Macro, Profile};
use apex5::tester::Gate;
use apex5::worker::{self, Cmd, Event, Handle};
use eframe::egui;

mod remember;
mod tray;
mod ui;

use tray::Wake;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Lighting,
    Screen,
    Profile,
    DualSense,
    Input,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProfileTab {
    Buttons,
    Macros,
    Sticks,
    Triggers,
    Vibration,
    Gyro,
}

#[derive(Clone)]
pub struct LedEdit {
    pub cfg: LedConfig,
    pub effect: Effect,
    pub colours: Vec<Rgb>,
    pub dirty_since: Option<Instant>,
}

/// Polling rate, sensor gates, then output gates and what games get now
/// (`None` without the gamepad node).
pub type Tester = (Option<f32>, [Gate; 2], Option<([Gate; 2], Output)>);

pub struct App {
    h: Handle,
    pub tab: Tab,
    pub profile_tab: ProfileTab,
    pub info: Option<Info>,
    pub status: String,
    pub toast: Option<(String, bool, Instant)>,
    pub busy: bool,
    pub settings: Option<Settings>,
    pub flags: Option<Flags>,
    pub led: Option<LedEdit>,
    /// Lighting written to the pad's working memory but not committed.
    pub led_flash_dirty: bool,
    /// The lighting as it was before this app changed it, or as last saved:
    /// what "Revert" brings back.
    pub led_saved: Option<LedEdit>,
    /// Profile as it is on the pad (RAM) and as being edited.
    pub pad_profile: Option<Profile>,
    pub edit: Option<Profile>,
    pub macros: Vec<Macro>,
    pub active: u8,
    pub flash_dirty: bool,
    pub nickname: String,
    pub title: String,
    pub streaming: bool,
    pub selected_key: u8,
    /// Rumble test strength, left and right, in %.
    pub rumble: (u8, u8),
    /// The input tester shows what games get rather than the sensor.
    pub tester_output: bool,
    /// "Reset all" asked once and waiting for the second press, since when.
    pub reset_armed: Option<Instant>,
    pub reset_lighting: bool,
    /// The profile menu's open dialog.
    pub copy_dialog: Option<ui::copies::Dialog>,
    pub save_name: String,
    /// Profiles saved on this computer, read when first shown.
    pub library: Option<Vec<apex5::library::Entry>>,
    pub load_lighting: bool,
    /// DualSense mode: % of the games' trigger resistance the pad gets.
    pub ds5_resistance: u8,
    /// Picture for the pad's screen and its upload.
    pub screen: ui::screen::State,
    /// What was asked of Flydigi's server.
    pub cloud: ui::cloud::State,
    shots: Option<Shots>,
    /// The window closes for good, even with DualSense mode on.
    quit: bool,
    /// The window's size when not maximized, and whether it is maximized:
    /// the next window opens the same.
    geometry: ([f32; 2], bool),
}

impl App {
    fn new(h: Handle, size: [f32; 2]) -> Self {
        let mut app = App {
            h,
            tab: Tab::Overview,
            profile_tab: ProfileTab::Buttons,
            info: None,
            status: t("Looking for the controller…").into(),
            toast: None,
            busy: false,
            settings: None,
            flags: None,
            led: None,
            led_flash_dirty: false,
            led_saved: None,
            pad_profile: None,
            edit: None,
            macros: vec![],
            active: 0,
            flash_dirty: false,
            nickname: String::new(),
            title: String::new(),
            streaming: false,
            selected_key: 4,
            rumble: (50, 50),
            tester_output: false,
            reset_armed: None,
            reset_lighting: true,
            copy_dialog: None,
            save_name: String::new(),
            library: None,
            load_lighting: true,
            screen: Default::default(),
            cloud: Default::default(),
            ds5_resistance: apex5::bridge::Settings::load().resistance,
            shots: Shots::from_env(),
            quit: false,
            geometry: (size, false),
        };
        if let Some(p) = std::env::var_os("APEX5_SCREEN_FILE") {
            ui::screen::preload(&mut app, p.into());
        }
        app
    }

    /// A window opens on the app: a new egui context, which needs the look
    /// set up and knows nothing of the last window's textures.
    fn attach(&mut self, ctx: &egui::Context) {
        ui::theme::install(ctx);
        self.screen.forget_textures();
        self.cloud.forget_textures();
    }

    fn options(&self) -> eframe::NativeOptions {
        let (size, maximized) = self.geometry;
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Apex 5 Control")
                .with_app_id("apex5-control")
                .with_inner_size(size)
                .with_maximized(maximized)
                .with_min_inner_size([820.0, 560.0])
                // the app draws its own title bar and window edges
                .with_decorations(false),
            ..Default::default()
        }
    }

    /// The window closed with DualSense mode on: the tray until the window
    /// is asked for again (true) or the app should end (false).
    fn background(&mut self, wake: &Receiver<Wake>, tx: &Sender<Wake>) -> bool {
        // knocks meant for the window that just closed
        while wake.try_recv().is_ok() {}
        let Some(_tray) = tray::Tray::show(tx.clone()) else {
            return false;
        };
        loop {
            match wake.recv_timeout(Duration::from_millis(200)) {
                Ok(Wake::Open) => return true,
                Ok(Wake::DualSenseOff) => {
                    self.send(Cmd::DualSense(false));
                    return false;
                }
                Ok(Wake::Quit) | Err(RecvTimeoutError::Disconnected) => return false,
                Err(RecvTimeoutError::Timeout) => {}
            }
            while let Ok(ev) = self.h.rx.try_recv() {
                self.on_event(ev);
            }
            // stopped some other way (an error): nothing to stay for
            if !self.ds5().on {
                return false;
            }
        }
    }

    /// Lets the worker put the pad back before the process ends: the
    /// virtual DualSense goes, trigger binds are undone, the stream is let
    /// go. It ends once nobody can send it commands.
    fn shutdown(&mut self) {
        drop(std::mem::replace(&mut self.h.tx, channel().0));
        // the worker holds the other half of `ds5` until it returns
        let until = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&self.h.ds5) > 1 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.h.tx.send(c);
    }

    /// DualSense mode as the worker last left it.
    pub fn ds5(&self) -> apex5::bridge::Status {
        self.h.ds5.lock().unwrap().clone()
    }

    pub fn input(&self) -> Option<apex5::device::InputState> {
        let g = self.h.input.lock().unwrap();
        g.last
            .as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_millis(500))
            .map(|(s, _)| *s)
    }

    pub fn tester(&self) -> Tester {
        let g = self.h.input.lock().unwrap();
        (g.rate, g.gates, g.out.map(|o| (g.out_gates, o)))
    }

    pub fn reset_gates(&self) {
        self.h.input.lock().unwrap().reset_gates();
    }

    /// The edited profile with the title and macro editors folded in.
    pub fn edited(&self) -> Option<Profile> {
        let mut e = self.edit.clone()?;
        e.set_title(&self.title);
        if e.macros() != self.macros {
            let _ = e.set_macros(&self.macros);
        }
        Some(e)
    }

    /// Lighting changed here and not yet saved in the pad.
    pub fn led_dirty(&self) -> bool {
        self.led_flash_dirty || self.led.as_ref().is_some_and(|l| l.dirty_since.is_some())
    }

    /// Commit the lighting to flash; a change still waiting for its write
    /// goes first.
    pub fn save_led(&mut self) {
        let pending = self
            .led
            .as_mut()
            .and_then(|l| l.dirty_since.take().map(|_| l.cfg.clone()));
        if let Some(cfg) = pending {
            self.send(Cmd::WriteLed(cfg));
        }
        self.send(Cmd::SaveLed);
    }

    /// Back to the lighting the pad keeps.
    pub fn revert_led(&mut self) {
        let Some(saved) = self.led_saved.clone() else {
            return;
        };
        if let Some(i) = &self.info {
            let choice = remember::Choice {
                effect: saved.effect,
                colours: saved.colours.clone(),
            };
            remember::store(&i.uid, saved.cfg.slot, choice);
        }
        self.send(Cmd::WriteLed(saved.cfg.clone()));
        self.led = Some(saved);
        self.led_flash_dirty = false;
    }

    pub fn profile_dirty(&self) -> bool {
        match (self.edited(), &self.pad_profile) {
            (Some(e), Some(p)) => e.blob != p.blob,
            _ => false,
        }
    }

    pub fn write_profile(&mut self) {
        let Some(mut e) = self.edit.clone() else {
            return;
        };
        if e.macros() != self.macros
            && let Err(err) = e.set_macros(&self.macros)
        {
            self.toast = Some((format!("{err}"), true, Instant::now()));
            return;
        }
        e.set_title(&self.title);
        self.edit = Some(e.clone());
        self.send(Cmd::WriteProfile(e));
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Connected(info) => {
                self.status = format!(
                    "{} · {}",
                    info.model(),
                    if info.wired { t("cable") } else { t("dongle") }
                );
                self.nickname = info.nickname.clone();
                self.info = Some(info);
            }
            Event::Disconnected(msg) => {
                // an upload lets go of the pad on purpose
                if self.info.is_some() && self.screen.job.is_none() {
                    self.toast = Some((t("Controller disconnected").into(), true, Instant::now()));
                }
                self.info = None;
                self.led_saved = None;
                self.settings = None;
                self.flags = None;
                self.led = None;
                self.pad_profile = None;
                self.edit = None;
                self.streaming = false;
                self.status = msg;
            }
            Event::Settings(s) => self.settings = Some(s),
            Event::Flags(f) => self.flags = Some(f),
            Event::Led(cfg) => {
                // What this editor last wrote, while the pad still plays it;
                // otherwise read back from the frames.
                let recalled = self
                    .info
                    .as_ref()
                    .and_then(|i| remember::recall(&i.uid, cfg.slot))
                    .filter(|c| cfg.made_by(c.effect, &c.colours));
                let (effect, mut colours): (Effect, Vec<Rgb>) = match recalled {
                    Some(c) => (c.effect, c.colours),
                    None => (cfg.effect(), cfg.colours()),
                };
                if colours.is_empty() || effect == Effect::Rainbow {
                    colours = vec![[0, 116, 255]];
                }
                if matches!(effect, Effect::Solid | Effect::Breath | Effect::Flash) {
                    colours.truncate(1);
                }
                // a slot read afresh is what the pad keeps, unless this app
                // changed it and did not save
                let other_slot = self
                    .led_saved
                    .as_ref()
                    .is_none_or(|s| s.cfg.slot != cfg.slot);
                let led = LedEdit {
                    cfg,
                    effect,
                    colours,
                    dirty_since: None,
                };
                if !self.led_flash_dirty || other_slot {
                    self.led_saved = Some(led.clone());
                }
                self.led = Some(led);
            }
            Event::LedSaved => {
                self.led_flash_dirty = false;
                self.led_saved = self.led.clone().map(|mut l| {
                    l.dirty_since = None;
                    l
                });
            }
            Event::Profile {
                profile,
                active,
                saved_tag,
            } => {
                let _ = saved_tag;
                self.flash_dirty = false;
                self.active = active;
                self.macros = profile.macros();
                self.title = profile.title();
                self.edit = Some(profile.clone());
                self.pad_profile = Some(profile);
            }
            Event::ProfileWritten(p) => {
                self.flash_dirty = true;
                self.pad_profile = Some(p);
            }
            Event::Screen(s) => ui::screen::on_stage(self, s),
            Event::ScreenDone(r) => ui::screen::on_done(self, r),
            Event::Busy(b) => self.busy = b,
            Event::Message(m) => self.toast = Some((m, false, Instant::now())),
            Event::Error(m) => self.toast = Some((m, true, Instant::now())),
        }
    }

    /// Accent colour follows the first colour the pad is lit with.
    fn accent_colour(&self) -> Option<[u8; 3]> {
        let led = self.led.as_ref()?;
        if led.effect == Effect::Off || led.cfg.brightness() == 0 {
            return None;
        }
        if led.effect == Effect::Rainbow {
            None
        } else {
            led.colours.first().copied()
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(ev) = self.h.rx.try_recv() {
            self.on_event(ev);
        }
        if ui::theme::set_accent_from_led(self.accent_colour()) {
            ui::theme::apply_style(ctx);
        }
        // Debounced live lighting writes.
        if let Some(led) = self.led.as_mut()
            && led
                .dirty_since
                .is_some_and(|t| t.elapsed() > Duration::from_millis(250))
            && !self.busy
        {
            led.dirty_since = None;
            let cfg = led.cfg.clone();
            if let Some(i) = &self.info {
                let choice = remember::Choice {
                    effect: led.effect,
                    colours: led.colours.clone(),
                };
                remember::store(&i.uid, cfg.slot, choice);
            }
            self.led_flash_dirty = true;
            let _ = self.h.tx.send(Cmd::WriteLed(cfg));
        }
        // Closing mid-upload would leave the screen without a picture.
        if self.screen.job.is_some() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.toast = Some((
                t("Wait until the picture finishes uploading").into(),
                true,
                Instant::now(),
            ));
        }
        let want_stream = (self.tab == Tab::Input
            || (self.tab == Tab::Profile && self.profile_tab == ProfileTab::Sticks))
            && self.info.is_some()
            && self.screen.job.is_none();
        if want_stream != self.streaming {
            self.streaming = want_stream;
            self.send(Cmd::Stream(want_stream));
        }
        if let Some(mut s) = self.shots.take() {
            if s.step(self, ctx) {
                self.shots = Some(s);
            } else {
                self.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        let (rect, maximized) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized));
        let maximized = maximized.unwrap_or(false);
        self.geometry.1 = maximized;
        if let Some(r) = rect.filter(|_| !maximized) {
            self.geometry.0 = r.size().into();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::shell(self, ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.streaming {
            self.streaming = false;
            self.send(Cmd::Stream(false));
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// One window's turn at the app, which outlives it.
struct Window<'a>(&'a mut App);

impl eframe::App for Window<'_> {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.0.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.0.ui(ui, frame);
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.0.on_exit(gl);
    }
}

/// `APEX5_SCREENSHOTS=<dir>`: visit every page, save a PNG of each, then quit.
/// `APEX5_SIZE=<w>x<h>` sets the window size, e.g. a tall one for whole pages.
/// `APEX5_SCREEN_FILE=<picture>` opens it on the screen page (nothing is sent).
/// It only switches pages: nothing is written to the pad except the input
/// stream toggle that the input page always uses.
struct Shots {
    dir: std::path::PathBuf,
    queue: Vec<(&'static str, Tab, ProfileTab)>,
    since: Instant,
    requested: bool,
}

impl Shots {
    fn from_env() -> Option<Self> {
        let dir = std::env::var_os("APEX5_SCREENSHOTS")?;
        let mut queue = vec![
            ("01-overview", Tab::Overview, ProfileTab::Buttons),
            ("02-lighting", Tab::Lighting, ProfileTab::Buttons),
            ("03-screen", Tab::Screen, ProfileTab::Buttons),
            ("04-buttons", Tab::Profile, ProfileTab::Buttons),
            ("05-macros", Tab::Profile, ProfileTab::Macros),
            ("06-sticks", Tab::Profile, ProfileTab::Sticks),
            ("07-triggers", Tab::Profile, ProfileTab::Triggers),
            ("08-vibration", Tab::Profile, ProfileTab::Vibration),
            ("09-gyro", Tab::Profile, ProfileTab::Gyro),
            ("10-dualsense", Tab::DualSense, ProfileTab::Buttons),
            ("11-input", Tab::Input, ProfileTab::Buttons),
            ("12-settings", Tab::Settings, ProfileTab::Buttons),
        ];
        queue.reverse();
        Some(Shots {
            dir: dir.into(),
            queue,
            since: Instant::now(),
            requested: false,
        })
    }

    /// Returns false when done.
    fn step(&mut self, app: &mut App, ctx: &egui::Context) -> bool {
        ctx.request_repaint_after(Duration::from_millis(50));
        let Some(&(name, tab, ptab)) = self.queue.last() else {
            return false;
        };
        // Wait for the pad's data before the first shot.
        if app.edit.is_none() && self.since.elapsed() < Duration::from_secs(8) {
            return true;
        }
        if app.tab != tab || app.profile_tab != ptab {
            app.tab = tab;
            app.profile_tab = ptab;
            self.since = Instant::now();
            self.requested = false;
            return true;
        }
        if !self.requested && self.since.elapsed() > Duration::from_millis(1200) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let path = self.dir.join(format!("{name}.png"));
            let [w, h] = img.size;
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                let _ = buf.save(&path);
            }
            self.queue.pop();
            self.since = Instant::now();
            self.requested = false;
            // Force a page switch next frame (same tab twice in a row is fine).
            if let Some(&(_, t, p)) = self.queue.last()
                && t == tab
                && p == ptab
            {
                self.requested = false;
            }
        }
        true
    }
}

fn main() -> eframe::Result {
    apex5::lang::load();
    let (tx, wake) = channel();
    let window = tray::Window::default();
    // a screenshot run leaves a running copy alone
    let _instance = if std::env::var_os("APEX5_SCREENSHOTS").is_some() {
        None
    } else {
        // another copy is running and shows its window instead
        let Some(i) = tray::Instance::claim(window.clone(), tx.clone()) else {
            return Ok(());
        };
        Some(i)
    };
    let size = std::env::var("APEX5_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([1240.0, 820.0]);
    let repaint = window.clone();
    let h = worker::spawn(move || {
        if let Some(ctx) = &*repaint.lock().unwrap() {
            ctx.request_repaint();
        }
    });
    let mut app = App::new(h, size);
    // Closing the window ends `run_native`; with DualSense mode on the app
    // stays in the tray and opens a new window when asked.
    let r = loop {
        let options = app.options();
        let (a, w) = (&mut app, &window);
        let r = eframe::run_native(
            "Apex 5 Control",
            options,
            Box::new(move |cc| {
                a.attach(&cc.egui_ctx);
                *w.lock().unwrap() = Some(cc.egui_ctx.clone());
                Ok(Box::new(Window(a)))
            }),
        );
        *window.lock().unwrap() = None;
        if r.is_err() || app.quit || !app.ds5().on || !app.background(&wake, &tx) {
            break r;
        }
    };
    app.shutdown();
    r
}
