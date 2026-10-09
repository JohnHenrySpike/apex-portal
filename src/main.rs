//! Apex 5 Control — native configuration app for the Flydigi Apex 5.

use std::time::{Duration, Instant};

use apex5::device::{Info, Settings};
use apex5::lighting::{Effect, LedConfig, Rgb};
use apex5::profile::{Macro, Profile};
use apex5::worker::{self, Cmd, Event, Handle};
use eframe::egui;

mod ui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Lighting,
    Profile,
    Settings,
    Input,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProfileTab {
    Buttons,
    Macros,
    Sticks,
    Triggers,
    Gyro,
}

pub struct LedEdit {
    pub cfg: LedConfig,
    pub effect: Effect,
    pub colours: Vec<Rgb>,
    pub dirty_since: Option<Instant>,
}

pub struct App {
    h: Handle,
    pub tab: Tab,
    pub profile_tab: ProfileTab,
    pub info: Option<Info>,
    pub status: String,
    pub toast: Option<(String, bool, Instant)>,
    pub busy: bool,
    pub settings: Option<Settings>,
    pub led: Option<LedEdit>,
    /// Lighting written to the pad's working memory but not committed.
    pub led_flash_dirty: bool,
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
    shots: Option<Shots>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        ui::theme::install(&ctx);
        App {
            h: worker::spawn(move || ctx.request_repaint()),
            tab: Tab::Overview,
            profile_tab: ProfileTab::Buttons,
            info: None,
            status: "Поиск геймпада…".into(),
            toast: None,
            busy: false,
            settings: None,
            led: None,
            led_flash_dirty: false,
            pad_profile: None,
            edit: None,
            macros: vec![],
            active: 0,
            flash_dirty: false,
            nickname: String::new(),
            title: String::new(),
            streaming: false,
            selected_key: 4,
            shots: Shots::from_env(),
        }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.h.tx.send(c);
    }

    pub fn input(&self) -> Option<apex5::device::InputState> {
        let g = self.h.input.lock().unwrap();
        g.as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_millis(500))
            .map(|(s, _)| *s)
    }

    /// The edited profile with the title and macro editors folded in.
    fn edited(&self) -> Option<Profile> {
        let mut e = self.edit.clone()?;
        e.set_title(&self.title);
        if e.macros() != self.macros {
            let _ = e.set_macros(&self.macros);
        }
        Some(e)
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
                    if info.wired {
                        "кабель"
                    } else {
                        "донгл"
                    }
                );
                self.nickname = info.nickname.clone();
                self.info = Some(info);
            }
            Event::Disconnected(msg) => {
                if self.info.is_some() {
                    self.toast = Some(("Геймпад отключился".into(), true, Instant::now()));
                }
                self.info = None;
                self.settings = None;
                self.led = None;
                self.pad_profile = None;
                self.edit = None;
                self.streaming = false;
                self.status = msg;
            }
            Event::Settings(s) => self.settings = Some(s),
            Event::Led(cfg) => {
                let effect = cfg.effect();
                let mut colours: Vec<Rgb> = vec![];
                for l in 0..cfg.leds() {
                    let c = cfg.led(0, l);
                    if c != [0, 0, 0] && !colours.contains(&c) && colours.len() < 5 {
                        colours.push(c);
                    }
                }
                if colours.is_empty() || effect == Effect::Rainbow {
                    colours = vec![[0, 116, 255]];
                }
                if matches!(effect, Effect::Solid | Effect::Breath | Effect::Flash) {
                    colours.truncate(1);
                }
                self.led = Some(LedEdit {
                    cfg,
                    effect,
                    colours,
                    dirty_since: None,
                });
            }
            Event::LedSaved => self.led_flash_dirty = false,
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
            self.led_flash_dirty = true;
            let _ = self.h.tx.send(Cmd::WriteLed(cfg));
        }
        let want_stream = (self.tab == Tab::Input
            || (self.tab == Tab::Profile && self.profile_tab == ProfileTab::Sticks))
            && self.info.is_some();
        if want_stream != self.streaming {
            self.streaming = want_stream;
            self.send(Cmd::Stream(want_stream));
        }
        if let Some(mut s) = self.shots.take() {
            if s.step(self, ctx) {
                self.shots = Some(s);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::shell(self, ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.streaming {
            self.send(Cmd::Stream(false));
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// `APEX5_SCREENSHOTS=<dir>`: visit every page, save a PNG of each, then quit.
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
            ("1-overview", Tab::Overview, ProfileTab::Buttons),
            ("2-lighting", Tab::Lighting, ProfileTab::Buttons),
            ("3-buttons", Tab::Profile, ProfileTab::Buttons),
            ("4-macros", Tab::Profile, ProfileTab::Macros),
            ("5-sticks", Tab::Profile, ProfileTab::Sticks),
            ("6-triggers", Tab::Profile, ProfileTab::Triggers),
            ("7-gyro", Tab::Profile, ProfileTab::Gyro),
            ("8-settings", Tab::Settings, ProfileTab::Buttons),
            ("9-input", Tab::Input, ProfileTab::Buttons),
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Apex 5 Control")
            .with_app_id("apex5-control")
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Apex 5 Control",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
