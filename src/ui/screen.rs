//! Own picture on the pad's screen: pick a file or one of Flydigi's, choose
//! the part that goes on the screen, see it as the screen will show it,
//! upload it, and the page that stays up while it goes.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use apex5::cloud::Picture;
use apex5::lang::t;
use apex5::screen::{self, Crop, Source, Stage, Upload};
use apex5::worker::Cmd;
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, pos2, vec2,
};
use image::imageops::{self, FilterType};

use super::theme::{self, accent};
use super::widgets::{self, hint, row};
use crate::App;

struct Draft {
    name: String,
    /// `None` for Flydigi's factory file, which is sent as it is.
    source: Option<Source>,
    /// The model type in a factory file's name.
    factory_type: Option<u8>,
    /// The part of the picture on the screen.
    crop: Crop,
    /// The crop `upload` was encoded with.
    encoded: Crop,
    interval_ms: u32,
    upload: Upload,
    /// Encoding failed.
    error: Option<String>,
    textures: Vec<TextureHandle>,
    /// While the crop moves, a GIF previews only the frame it was on.
    live: Option<Live>,
    /// The editor's picture: the first frame.
    picture: Option<TextureHandle>,
    /// What the editor shows, in fractions of the picture; it follows the
    /// crop once the crop rests.
    view: Option<Rect>,
    /// A drag in the editor, as it began.
    grip: Option<Grip>,
    /// When the crop last changed by hand.
    touched: Option<Instant>,
    /// Just opened from the gallery below: scroll up to it once.
    reveal: bool,
}

struct Live {
    crop: Crop,
    frame: usize,
    texture: TextureHandle,
}

#[derive(Clone, Copy)]
struct Grip {
    from: Crop,
    origin: Pos2,
    /// The corner held, as (right, bottom); `None` moves the whole frame.
    corner: Option<(bool, bool)>,
}

/// A GIF encodes all its frames once the crop has rested this long.
const SETTLE: Duration = Duration::from_millis(300);

impl Draft {
    fn new(name: String, source: Option<Source>, upload: Upload, interval_ms: u32) -> Self {
        let crop = source.as_ref().map_or(Crop::fill(1, 1), |s| {
            let (w, h) = s.frames[0].dimensions();
            Crop::fill(w, h)
        });
        Draft {
            name,
            source,
            factory_type: None,
            crop,
            encoded: crop,
            interval_ms,
            upload,
            error: None,
            textures: vec![],
            live: None,
            picture: None,
            view: None,
            grip: None,
            touched: None,
            reveal: false,
        }
    }

    /// The decoded picture's size in pixels.
    fn size(&self) -> (u32, u32) {
        self.source
            .as_ref()
            .map_or((1, 1), |s| s.frames[0].dimensions())
    }

    /// The crop is being changed: a drag, or the wheel a moment ago.
    fn moving(&self) -> bool {
        self.grip.is_some() || self.touched.is_some_and(|t| t.elapsed() < SETTLE)
    }

    /// Encodes every frame with the current crop.
    fn settle(&mut self) {
        self.live = None;
        let Some(src) = &self.source else { return };
        if self.crop == self.encoded {
            return;
        }
        match encode(src, self.crop, self.interval_ms) {
            Ok(u) => {
                self.upload = u;
                self.textures.clear();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        self.encoded = self.crop;
    }
}

pub struct Job {
    /// `None` while the worker still checks the cable and the udev rule.
    pub stage: Option<Stage>,
    /// When the stage began.
    since: Instant,
}

#[derive(Default)]
pub struct State {
    draft: Option<Draft>,
    loading: Option<Receiver<Result<Draft, String>>>,
    armed: Option<Instant>,
    pub job: Option<Job>,
    /// The last upload's outcome, and whether it failed.
    result: Option<(String, bool)>,
    rule: Option<(bool, Instant)>,
    /// The udev rule being installed through pkexec.
    installing: Option<Receiver<Result<(), String>>>,
}

impl State {
    /// A new window has a new egui context: the old one's textures mean
    /// nothing there and are made again when drawn.
    pub fn forget_textures(&mut self) {
        if let Some(d) = &mut self.draft {
            d.textures.clear();
            d.live = None;
            d.picture = None;
        }
    }
}

fn encode(src: &Source, crop: Crop, interval_ms: u32) -> anyhow::Result<Upload> {
    let frames: Vec<Vec<u8>> = src
        .frames
        .iter()
        .map(|f| screen::encode_crop(f, crop))
        .collect();
    Upload::new(&frames, interval_ms)
}

fn load(path: PathBuf) -> anyhow::Result<Draft> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
    {
        let upload = Upload::factory(std::fs::read(&path)?)?;
        let mut d = Draft::new(name, None, upload, 100);
        d.factory_type = d
            .name
            .strip_prefix("default_screen_image_")
            .and_then(|r| r.strip_suffix(".bin"))
            .and_then(|t| t.parse().ok());
        return Ok(d);
    }
    let src = screen::decode(&path)?;
    let interval_ms = src.interval_ms.unwrap_or(100);
    let (w, h) = src.frames[0].dimensions();
    let upload = encode(&src, Crop::fill(w, h), interval_ms)?;
    Ok(Draft::new(name, Some(src), upload, interval_ms))
}

/// Opens a picture straight away, for the screenshot run.
pub fn preload(app: &mut App, path: PathBuf) {
    app.screen.draft = load(path).ok();
}

/// The file dialog and decoding run off the UI thread.
fn pick(app: &mut App, ctx: &egui::Context) {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let file = rfd::FileDialog::new()
            .set_title(t("Picture for the controller's screen"))
            .add_filter(t("Picture or GIF"), &["png", "jpg", "jpeg", "gif"])
            .add_filter(t("Flydigi factory animation"), &["bin"])
            .pick_file();
        if let Some(p) = file {
            let _ = tx.send(load(p).map_err(|e| format!("{e:#}")));
        }
        ctx.request_repaint();
    });
    app.screen.loading = Some(rx);
}

/// One of Flydigi's pictures: downloaded once, then opened like a file.
fn pick_official(app: &mut App, ctx: &egui::Context, p: Picture) {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let r = p.fetch().and_then(load).map(|mut d| {
            d.name = format!("{} (Flydigi)", p.title);
            d.reveal = true;
            d
        });
        let _ = tx.send(r.map_err(|e| format!("{e:#}")));
        ctx.request_repaint();
    });
    app.screen.loading = Some(rx);
}

fn poll(app: &mut App) {
    let Some(rx) = &app.screen.loading else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(d)) => {
            app.screen.draft = Some(d);
            app.screen.result = None;
            app.screen.loading = None;
        }
        Ok(Err(e)) => {
            app.screen.result = Some((apex5::tr!("Couldn't open: {e}", e), true));
            app.screen.loading = None;
        }
        Err(TryRecvError::Disconnected) => app.screen.loading = None, // cancelled
        Err(TryRecvError::Empty) => {}
    }
}

/// Installs the udev rules; the system asks for the password.
fn install_rule(app: &mut App, ctx: &egui::Context) {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(apex5::udev::install().map_err(|e| format!("{e:#}")));
        ctx.request_repaint();
    });
    app.screen.installing = Some(rx);
}

fn poll_install(app: &mut App) {
    let Some(rx) = &app.screen.installing else {
        return;
    };
    let r = match rx.try_recv() {
        Ok(r) => r,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err(t("the install was interrupted").into()),
    };
    app.screen.installing = None;
    app.screen.rule = None;
    app.toast = Some(match r {
        Ok(()) => (t("udev rule installed").into(), false, Instant::now()),
        Err(e) => (
            apex5::tr!("Rule not installed: {e}", e),
            true,
            Instant::now(),
        ),
    });
}

/// "Install the rule" with a spinner while the system prompt is up.
/// Returns whether it was pressed.
fn rule_button(ui: &mut egui::Ui, busy: bool) -> bool {
    ui.horizontal(|ui| {
        let r = widgets::secondary(ui, t("Install rule…"), !busy).on_hover_text(
            apex5::tr!("Writes {path} and reloads the udev rules. The system asks for the password, not the app", path = apex5::udev::PATH),
        );
        if busy {
            ui.add(egui::Spinner::new().color(accent()));
        }
        r.clicked()
    })
    .inner
}

/// For the "no access" page: the same rule gives access to the pad itself.
pub fn rule_offer(app: &mut App, ui: &mut egui::Ui) {
    poll_install(app);
    if rule_button(ui, app.screen.installing.is_some()) {
        install_rule(app, ui.ctx());
    }
}

fn rule_ok(app: &mut App) -> bool {
    match app.screen.rule {
        Some((ok, at)) if at.elapsed() < Duration::from_secs(5) => ok,
        _ => {
            let ok = screen::rule_installed();
            app.screen.rule = Some((ok, Instant::now()));
            ok
        }
    }
}

pub fn on_stage(app: &mut App, s: Stage) {
    let Some(job) = app.screen.job.as_mut() else {
        return;
    };
    let same = job
        .stage
        .as_ref()
        .is_some_and(|old| std::mem::discriminant(old) == std::mem::discriminant(&s));
    if !same {
        job.since = Instant::now();
    }
    job.stage = Some(s);
}

pub fn on_done(app: &mut App, r: Result<(), String>) {
    let started = app.screen.job.take().is_some_and(|j| j.stage.is_some());
    let (msg, err) = match r {
        Ok(()) => {
            app.screen.draft = None;
            (t("Picture uploaded").to_string(), false)
        }
        Err(e) if started => (
            apex5::tr!(
                "Not uploaded: {e}. Try again: the screen waits for the upload until the controller \
                 is turned off. If it doesn't work, turn the controller off and on with the switch \
                 on its back",
                e
            ),
            true,
        ),
        Err(e) => (apex5::tr!("Not uploaded: {e}", e), true),
    };
    app.toast = Some((msg.clone(), err, Instant::now()));
    app.screen.result = Some((msg, err));
}

/// What a change on the page asks for, applied once the draft is let go.
enum Action {
    Pick,
    Official(Picture),
    InstallRule,
    Upload(Upload),
    Clear,
}

/// The "Screen" page: the screen switches, own picture and Flydigi's.
pub fn page(app: &mut App, ui: &mut egui::Ui) {
    super::header(
        app,
        ui,
        t("Screen"),
        t("The picture on the controller's screen"),
    );
    poll(app);
    poll_install(app);
    widgets::card_titled(ui, t("Screen"), |ui| switches(app, ui));
    let mut action = None;
    ui.add_space(12.0);
    widgets::card_titled(ui, t("Own picture"), |ui| own(app, ui, &mut action));
    ui.add_space(12.0);
    widgets::card_titled(ui, t("Flydigi's collection"), |ui| {
        flydigi(app, ui, &mut action)
    });
    match action {
        Some(Action::Pick) => pick(app, ui.ctx()),
        Some(Action::Official(p)) => pick_official(app, ui.ctx(), p),
        Some(Action::InstallRule) => install_rule(app, ui.ctx()),
        Some(Action::Upload(up)) => {
            app.screen.result = None;
            app.screen.job = Some(Job {
                stage: None,
                since: Instant::now(),
            });
            app.send(Cmd::UploadScreen(up));
        }
        Some(Action::Clear) => app.screen.draft = None,
        None => {}
    }
}

fn switches(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = app.settings.clone() else {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    };
    let mut first = true;
    for (sub, name, hint_text, more) in [
        (
            9u8,
            t("Screen always on"),
            t("Don't turn the screen off while playing"),
            "",
        ),
        (
            8,
            t("Status bar always visible"),
            t("Battery and profile over the picture"),
            t(
                "Experimental: Flydigi Space Station doesn't show this setting and turns it off \
                 itself on every connection",
            ),
        ),
    ] {
        if !s.is_supported(sub) {
            continue;
        }
        if !first {
            widgets::separator(ui);
        }
        first = false;
        let mut on = s.is_on(sub);
        let r = ui.scope(|ui| widgets::toggle_row(ui, name, hint_text, &mut on));
        if !more.is_empty() {
            r.response.on_hover_text(more);
        }
        if r.inner {
            app.send(Cmd::SetFeature(sub, on));
        }
    }
}

/// The own picture card: the file, the editor, the upload.
fn own(app: &mut App, ui: &mut egui::Ui, action: &mut Option<Action>) {
    let loading = app.screen.loading.is_some();
    ui.horizontal_wrapped(|ui| {
        let r = widgets::secondary(ui, t("Choose file…"), !loading && !app.busy).on_hover_text(t(
            "The controller's screen is 160 × 80. The factory animation comes back with the file \
                 default_screen_image_<type>.bin from Flydigi Space Station",
        ));
        if r.clicked() {
            *action = Some(Action::Pick);
        }
        if loading {
            ui.add(egui::Spinner::new().color(accent()));
        }
        hint(ui, t("PNG, JPEG or GIF up to 150 frames"));
    });
    if let Some((msg, err)) = &app.screen.result {
        ui.add_space(4.0);
        ui.label(RichText::new(msg).color(if *err { theme::bad() } else { theme::ok() }));
    }

    let wired = app.info.as_ref().is_some_and(|i| i.wired);
    let device_type = app.info.as_ref().map(|i| i.device_type);
    let rule = app.screen.draft.is_some() && rule_ok(app);
    let installing = app.screen.installing.is_some();
    let busy = app.busy;
    const WAIT: Duration = Duration::from_secs(5);
    let armed = app.screen.armed.is_some_and(|t| t.elapsed() < WAIT);
    let mut arm = None;

    if let Some(d) = app.screen.draft.as_mut() {
        ui.add_space(12.0);
        let top = ui.cursor().min.y;
        if d.source.is_some() {
            // the editor beside the result, or above it when narrow
            let w = ui.available_width();
            if w >= 720.0 {
                let left = (w - 14.0 - PREVIEW_W) / (w - 14.0);
                widgets::split(ui, left, d, edit_column, result_column);
            } else {
                edit_column(d, ui);
                ui.add_space(14.0);
                result_column(d, ui);
            }
        } else {
            result_column(d, ui);
        }
        if d.reveal {
            d.reveal = false;
            let at = Rect::from_min_size(pos2(ui.min_rect().left(), top - 60.0), vec2(1.0, 1.0));
            ui.scroll_to_rect(at, Some(egui::Align::Min));
        }

        ui.add_space(12.0);
        let wrong_model = d.source.is_none() && d.factory_type != device_type;
        let blocker = if !wired {
            Some(
                t("Connect the controller by cable: uploading through the dongle isn't possible")
                    .to_string(),
            )
        } else if !rule {
            Some(screen::rule_hint().to_string())
        } else if wrong_model {
            Some(match device_type {
                Some(ty) => {
                    apex5::tr!(
                        "Needs the file default_screen_image_{ty}.bin: this one is for another model",
                        ty
                    )
                }
                None => t("Controller not connected").to_string(),
            })
        } else {
            d.error.clone()
        };
        match &blocker {
            Some(b) => {
                ui.label(RichText::new(b).color(theme::warn()));
                if wired && !rule {
                    ui.add_space(4.0);
                    if rule_button(ui, installing) {
                        *action = Some(Action::InstallRule);
                    }
                }
            }
            None => {
                ui.label(
                    RichText::new(t("Don't unplug the cable while uploading"))
                        .size(12.5)
                        .color(theme::dim()),
                )
                .on_hover_text(t(
                    "While uploading, don't unplug the cable or turn the controller off. \
                     The app can't be used meanwhile. At the end the controller \
                     restarts.",
                ));
            }
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let text = if armed {
                t("Upload for sure? Press again")
            } else {
                t("Upload to controller")
            };
            if widgets::primary(ui, text, blocker.is_none() && !busy).clicked() {
                if armed {
                    arm = Some(None);
                    // a GIF still resting from a drag
                    d.settle();
                    *action = Some(Action::Upload(d.upload.clone()));
                } else {
                    arm = Some(Some(Instant::now()));
                }
            }
            if armed {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
                if ui.button(t("Cancel")).clicked() {
                    arm = Some(None);
                }
            } else if widgets::secondary(ui, t("Remove"), true).clicked() {
                *action = Some(Action::Clear);
            }
        });
    }
    if let Some(a) = arm {
        app.screen.armed = a;
    }
}

/// The Flydigi gallery card: shown on request, it goes to Flydigi's server.
fn flydigi(app: &mut App, ui: &mut egui::Ui, action: &mut Option<Action>) {
    let open = app.cloud.gallery_open;
    let loading = app.screen.loading.is_some();
    let sub = if loading && open {
        t("Opening…")
    } else if open {
        t("Click a picture to open it above")
    } else {
        t("Downloaded from Flydigi's server")
    };
    let button = if open { t("Hide") } else { t("Show") };
    let toggle = row(ui, t("Flydigi pictures and animations"), sub, |ui| {
        widgets::secondary(ui, button, true).clicked()
    });
    if toggle {
        app.cloud.gallery_open = !open;
    }
    if app.cloud.gallery_open {
        ui.add_space(8.0);
        let enabled = !loading && !app.busy;
        if let Some(p) = super::cloud::gallery(&mut app.cloud, enabled, ui) {
            *action = Some(Action::Official(p));
        }
    }
}

// ------------------------------------------------------------------ editor

/// The preview's width: the screen at twice the size in its frame.
const PREVIEW_W: f32 = 2.0 * screen::W as f32 + 14.0;

/// The editor, its quick buttons, and the preview kept up with it.
fn edit_column(d: &mut Draft, ui: &mut egui::Ui) {
    editor(ui, d);
    ui.add_space(8.0);
    let (w, h) = d.size();
    let mut set = None;
    ui.horizontal_wrapped(|ui| {
        if widgets::secondary(ui, t("Fill"), true)
            .on_hover_text(t("The picture fills the screen, the rest is cut off"))
            .clicked()
        {
            set = Some(Crop::fill(w, h));
        }
        if widgets::secondary(ui, t("Fit"), true)
            .on_hover_text(t("The whole picture, black bars at the edges"))
            .clicked()
        {
            set = Some(Crop::whole(w, h));
        }
        if widgets::secondary(ui, t("Center#crop"), true)
            .on_hover_text(t("A frame of the same size in the middle of the picture"))
            .clicked()
        {
            set = Some(d.crop.centered());
        }
    });
    hint(
        ui,
        t("Drag the frame or its corners, the mouse wheel zooms"),
    );
    if let Some(c) = set {
        d.crop = c;
        d.touched = None;
    }
    let n = d.textures.len();
    let shown = shown_frame(ui, n, d.interval_ms);
    refresh(ui.ctx(), d, shown);
}

/// Maps the editor's fractions of the picture onto the screen and back.
struct Map {
    center: Pos2,
    view_center: Pos2,
    /// Points per whole picture width and height.
    scale: egui::Vec2,
}

impl Map {
    /// `view` fitted into `inner`.
    fn new(inner: Rect, view: Rect, (w, h): (u32, u32)) -> Map {
        let s = (inner.width() / (view.width() * w as f32))
            .min(inner.height() / (view.height() * h as f32));
        Map {
            center: inner.center(),
            view_center: view.center(),
            scale: vec2(s * w as f32, s * h as f32),
        }
    }

    fn to_screen(&self, p: Pos2) -> Pos2 {
        self.center + (p - self.view_center) * self.scale
    }

    fn to_picture(&self, p: Pos2) -> Pos2 {
        self.view_center + (p - self.center) / self.scale
    }

    fn rect(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }
}

fn crop_rect(c: Crop) -> Rect {
    Rect::from_min_size(pos2(c.x, c.y), vec2(c.w, c.h))
}

/// The crop after dragging `g` to `p`: moved, or resized about the
/// opposite corner, 2 : 1 kept.
fn dragged(g: Grip, p: Pos2, map: &Map, (w, h): (u32, u32)) -> Crop {
    let f = g.from;
    let Some((right, bottom)) = g.corner else {
        let d = (p - g.origin) / map.scale;
        return Crop {
            x: f.x + d.x,
            y: f.y + d.y,
            ..f
        }
        .limited(w, h);
    };
    let (lo, hi) = Crop::width_range(w, h);
    let ax = if right { f.x } else { f.x + f.w };
    let ay = if bottom { f.y } else { f.y + f.h };
    let q = map.to_picture(p);
    let by_x = if right { q.x - ax } else { ax - q.x };
    let by_y = (if bottom { q.y - ay } else { ay - q.y }) * f.w / f.h;
    let cw = by_x.max(by_y).clamp(lo, hi);
    let ch = cw * f.h / f.w;
    Crop {
        x: if right { ax } else { ax - cw },
        y: if bottom { ay } else { ay - ch },
        w: cw,
        h: ch,
    }
    .limited(w, h)
}

fn resize_icon((right, bottom): (bool, bool)) -> CursorIcon {
    if right == bottom {
        CursorIcon::ResizeNwSe
    } else {
        CursorIcon::ResizeNeSw
    }
}

/// The picture with the screen's frame over it and the rest dimmed: drag
/// the frame or its corners, the wheel zooms about the pointer. The frame
/// may leave the picture; the screen is black there.
fn editor(ui: &mut egui::Ui, d: &mut Draft) {
    let Some(src) = &d.source else { return };
    let (w, h) = d.size();
    if d.picture.is_none() {
        // the editor never needs more than this
        let first = &src.frames[0];
        let k = 1600.0 / w.max(h) as f32;
        let img = if k < 1.0 {
            let (nw, nh) = ((w as f32 * k).round() as u32, (h as f32 * k).round() as u32);
            imageops::resize(first, nw.max(1), nh.max(1), FilterType::Triangle)
        } else {
            first.clone()
        };
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        );
        d.picture = Some(
            ui.ctx()
                .load_texture("screen-editor", img, TextureOptions::LINEAR),
        );
    }

    let width = ui.available_width();
    let height = (width * h as f32 / w as f32).clamp(160.0, 460.0);
    let (rect, r) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());

    // the view takes in the picture and the frame, eased once the frame rests
    let target = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)).union(crop_rect(d.crop));
    let view = match d.view {
        None => target,
        Some(v) if d.moving() => v,
        Some(v) => {
            let t = 1.0 - (-ui.input(|i| i.stable_dt).min(0.1) * 14.0).exp();
            let v = v.lerp_towards(&target, t);
            if (v.min - target.min).length() + (v.max - target.max).length() > 1e-3 {
                ui.ctx().request_repaint();
                v
            } else {
                target
            }
        }
    };
    d.view = Some(view);
    let map = Map::new(rect.shrink(14.0), view, (w, h));
    let corners = [(false, false), (true, false), (false, true), (true, true)];
    let corner_at = |p: Pos2, c: Crop| {
        let cr = map.rect(crop_rect(c));
        let reach = (cr.width() / 3.0).min(12.0);
        corners.into_iter().find(|&(right, bottom)| {
            let q = pos2(
                if right { cr.max.x } else { cr.min.x },
                if bottom { cr.max.y } else { cr.min.y },
            );
            q.distance(p) < reach
        })
    };

    let mut crop = d.crop;
    if r.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        d.grip = Some(Grip {
            from: d.crop,
            origin,
            corner: corner_at(origin, d.crop),
        });
    }
    if let Some(g) = d.grip {
        match r.interact_pointer_pos() {
            Some(p) if r.dragged() => crop = dragged(g, p, &map, (w, h)),
            _ => d.grip = None,
        }
    }
    let hover = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|_| ui.rect_contains_pointer(rect));
    if let Some(p) = hover {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        if scroll != 0.0 || zoom != 1.0 {
            // the page does not scroll under the editor
            ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            let q = map.to_picture(p);
            crop = crop.zoomed((-scroll / 400.0).exp() / zoom, (q.x, q.y), w, h);
        }
    }
    if crop != d.crop {
        d.crop = crop;
        d.touched = Some(Instant::now());
    }
    if let Some(t) = d.touched
        && t.elapsed() < SETTLE
    {
        ui.ctx()
            .request_repaint_after(SETTLE - t.elapsed() + Duration::from_millis(10));
    }
    let icon = match d.grip.map(|g| g.corner) {
        Some(Some(c)) => Some(resize_icon(c)),
        Some(None) => Some(CursorIcon::Grabbing),
        None => hover.map(|p| corner_at(p, d.crop).map_or(CursorIcon::Grab, resize_icon)),
    };
    if let Some(i) = icon {
        ui.ctx().set_cursor_icon(i);
    }

    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_rgb(13, 15, 19));
    let cr = map.rect(crop_rect(d.crop));
    // black where the frame leaves the picture, as on the screen
    p.rect_filled(cr, 0.0, Color32::BLACK);
    let pic = map.rect(Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
    if let Some(t) = &d.picture {
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        p.image(t.id(), pic, uv, Color32::WHITE);
    }
    p.rect_stroke(
        pic,
        0.0,
        Stroke::new(1.0, Color32::from_white_alpha(28)),
        StrokeKind::Outside,
    );
    let dim = Color32::from_black_alpha(170);
    let c = cr.intersect(rect);
    if c.is_positive() {
        for part in [
            Rect::from_min_max(rect.min, pos2(rect.max.x, c.min.y)),
            Rect::from_min_max(pos2(rect.min.x, c.max.y), rect.max),
            Rect::from_min_max(pos2(rect.min.x, c.min.y), pos2(c.min.x, c.max.y)),
            Rect::from_min_max(pos2(c.max.x, c.min.y), pos2(rect.max.x, c.max.y)),
        ] {
            if part.is_positive() {
                p.rect_filled(part, 0.0, dim);
            }
        }
    } else {
        p.rect_filled(rect, 0.0, dim);
    }
    p.rect_stroke(cr, 0.0, Stroke::new(1.5, accent()), StrokeKind::Outside);
    for (right, bottom) in corners {
        let q = pos2(
            if right { cr.max.x } else { cr.min.x },
            if bottom { cr.max.y } else { cr.min.y },
        );
        p.rect(
            Rect::from_center_size(q, vec2(9.0, 9.0)),
            CornerRadius::same(2),
            accent(),
            Stroke::new(1.5, Color32::BLACK),
            StrokeKind::Outside,
        );
    }
    p.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );
}

/// Keeps the preview up with the crop: a still at once; a GIF by the one
/// frame it shows while the crop moves, every frame once the crop rests.
fn refresh(ctx: &egui::Context, d: &mut Draft, shown: usize) {
    let Some(frames) = d.source.as_ref().map(|s| s.frames.len()) else {
        return;
    };
    if d.crop == d.encoded {
        d.live = None;
        return;
    }
    if frames == 1 || !d.moving() {
        d.settle();
        return;
    }
    if d.live.as_ref().is_some_and(|l| l.crop == d.crop) {
        return;
    }
    let frame = d.live.as_ref().map_or(shown.min(frames - 1), |l| l.frame);
    let src = d.source.as_ref().unwrap();
    let img = color_image(&screen::encode_crop(&src.frames[frame], d.crop));
    match &mut d.live {
        Some(l) => {
            l.texture.set(img, TextureOptions::NEAREST);
            l.crop = d.crop;
        }
        None => {
            d.live = Some(Live {
                crop: d.crop,
                frame,
                texture: ctx.load_texture("screen-live", img, TextureOptions::NEAREST),
            })
        }
    }
}

fn color_image(frame: &[u8]) -> egui::ColorImage {
    let rgba = screen::to_rgba(frame);
    egui::ColorImage::from_rgba_unmultiplied(
        [screen::W as usize, screen::H as usize],
        rgba.as_raw(),
    )
}

/// The frame of an `n`-frame animation playing now.
fn shown_frame(ui: &egui::Ui, n: usize, interval_ms: u32) -> usize {
    if n < 2 {
        return 0;
    }
    let ms = (ui.input(|i| i.time) * 1000.0) as u64;
    super::repaint_every(ui.ctx(), Duration::from_millis(interval_ms as u64));
    (ms / interval_ms.max(10) as u64) as usize % n
}

/// The preview with what it is beside it, or under it when narrow.
fn result_column(d: &mut Draft, ui: &mut egui::Ui) {
    if ui.available_width() >= PREVIEW_W + 14.0 + 170.0 {
        ui.horizontal_top(|ui| {
            preview(ui, d);
            ui.add_space(14.0);
            ui.vertical(|ui| about(ui, d));
        });
    } else {
        preview(ui, d);
        ui.add_space(8.0);
        about(ui, d);
    }
}

/// The name, frames, speed and how long the upload takes.
fn about(ui: &mut egui::Ui, d: &mut Draft) {
    ui.add(egui::Label::new(RichText::new(&d.name).color(theme::text())).truncate());
    let n = d.upload.frames;
    let what = match (&d.source, n) {
        (None, _) => apex5::tr!("Flydigi factory animation, frames: {n}", n),
        (Some(_), 1) => t("Picture").to_string(),
        (Some(_), _) => apex5::tr!("Animation, frames: {n}", n),
    };
    hint(ui, &what);
    if d.source.as_ref().is_some_and(|s| s.cut) {
        ui.label(
            RichText::new(apex5::tr!(
                "The GIF has more than {max} frames, the first {max} are taken",
                max = screen::MAX_FRAMES
            ))
            .size(12.5)
            .color(theme::warn()),
        );
    }
    if d.source.is_some() && n > 1 {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Frame")).color(theme::dim()));
            let r = ui.add(
                egui::DragValue::new(&mut d.interval_ms)
                    .range(screen::INTERVAL_MS.0..=screen::INTERVAL_MS.1)
                    .speed(2.0)
                    .suffix(t(" ms")),
            );
            if r.changed() {
                d.upload.set_interval(d.interval_ms);
            }
        });
    }
    ui.add_space(4.0);
    let minutes = (n as u32 * screen::SECONDS_PER_FRAME).div_ceil(60);
    hint(
        ui,
        &apex5::tr!("The upload takes about {minutes} min", minutes),
    );
    if let Some(e) = &d.error {
        ui.label(RichText::new(e).color(theme::bad()));
    }
}

/// The frames as the screen will show them, at twice the size.
fn preview(ui: &mut egui::Ui, d: &mut Draft) {
    if d.textures.len() != d.upload.frames {
        d.textures = (0..d.upload.frames)
            .map(|i| {
                ui.ctx().load_texture(
                    format!("screen-{i}"),
                    color_image(d.upload.frame(i)),
                    TextureOptions::NEAREST,
                )
            })
            .collect();
    }
    let tex = match &d.live {
        Some(l) => &l.texture,
        None => &d.textures[shown_frame(ui, d.textures.len(), d.interval_ms)],
    };
    let scale = ((ui.available_width() - 14.0) / screen::W as f32).clamp(1.0, 2.0);
    let size = vec2(screen::W as f32 * scale, screen::H as f32 * scale);
    egui::Frame::new()
        .fill(Color32::BLACK)
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(size)
                    .corner_radius(CornerRadius::same(4)),
            );
        });
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

/// Shown instead of everything else while an upload runs.
pub fn job_page(app: &App, ui: &mut egui::Ui) {
    let Some(job) = &app.screen.job else {
        return;
    };
    ui.ctx().request_repaint_after(Duration::from_millis(500));
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.set_max_width(520.0);
        ui.label(
            RichText::new(t("Uploading the picture to the screen"))
                .font(theme::bold(24.0))
                .color(theme::text()),
        );
        ui.add_space(16.0);
        match &job.stage {
            None => {
                ui.add(egui::Spinner::new().size(22.0).color(accent()));
                ui.label(RichText::new(t("Checking the connection…")).color(theme::dim()));
            }
            Some(Stage::Switching) => {
                ui.add(egui::Spinner::new().size(22.0).color(accent()));
                ui.label(RichText::new(t("The screen is switching to upload mode…")).color(theme::dim()));
            }
            Some(Stage::Writing(done, total)) => {
                let f = *done as f32 / (*total).max(1) as f32;
                ui.add(
                    egui::ProgressBar::new(f)
                        .fill(accent())
                        .desired_height(10.0)
                        .corner_radius(CornerRadius::same(5)),
                );
                ui.add_space(8.0);
                let spent = job.since.elapsed();
                let left = if *done > 0 {
                    let rest = spent.mul_f64((*total - *done) as f64 / *done as f64);
                    apex5::tr!(", about {left} left", left = clock(rest))
                } else {
                    String::new()
                };
                ui.label(
                    RichText::new(apex5::tr!("{percent}% · {spent} elapsed{left}", percent = format!("{:.0}", f * 100.0), spent = clock(spent), left))
                    .color(theme::dim()),
                );
            }
            Some(Stage::Saving) => {
                ui.add(egui::Spinner::new().size(22.0).color(accent()));
                ui.label(
                    RichText::new(t("The screen is saving the picture, then the controller restarts. About 20 seconds."))
                    .color(theme::dim()),
                );
            }
        }
        ui.add_space(20.0);
        ui.label(
            RichText::new(t("Don't unplug the cable, turn the controller off or close the app."))
            .color(theme::warn()),
        );
        ui.label(
            RichText::new(t("An interrupted upload leaves the screen without a picture, but it can be repeated."))
                .size(12.5)
                .color(theme::faint()),
        );
    });
}
