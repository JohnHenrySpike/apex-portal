//! Own picture on the pad's screen: pick a file, see it as the screen will
//! show it, upload it, and the page that stays up while it goes.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use apex5::screen::{self, Fit, Source, Stage, Upload};
use apex5::worker::Cmd;
use eframe::egui::{self, CornerRadius, RichText, Stroke, TextureHandle, vec2};

use super::theme::{self, BAD, BORDER, DIM, FAINT, OK, TEXT, WARN, accent};
use super::widgets::{self, hint, row};
use crate::App;

pub struct Draft {
    name: String,
    /// `None` for Flydigi's factory file, which is sent as it is.
    source: Option<Source>,
    /// The model type in a factory file's name.
    factory_type: Option<u8>,
    fit: Fit,
    interval_ms: u32,
    upload: Upload,
    textures: Vec<TextureHandle>,
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

fn encode(src: &Source, fit: Fit, interval_ms: u32) -> anyhow::Result<Upload> {
    let frames: Vec<Vec<u8>> = src.frames.iter().map(|f| screen::encode(f, fit)).collect();
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
        let factory_type = name
            .strip_prefix("default_screen_image_")
            .and_then(|r| r.strip_suffix(".bin"))
            .and_then(|t| t.parse().ok());
        return Ok(Draft {
            name,
            source: None,
            factory_type,
            fit: Fit::Fill,
            interval_ms: 100,
            upload,
            textures: vec![],
        });
    }
    let src = screen::decode(&path)?;
    let interval_ms = src.interval_ms.unwrap_or(100);
    let upload = encode(&src, Fit::Fill, interval_ms)?;
    Ok(Draft {
        name,
        source: Some(src),
        factory_type: None,
        fit: Fit::Fill,
        interval_ms,
        upload,
        textures: vec![],
    })
}

/// The file dialog and decoding run off the UI thread.
fn pick(app: &mut App, ctx: &egui::Context) {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let file = rfd::FileDialog::new()
            .set_title("Картинка для экрана геймпада")
            .add_filter("Картинка или GIF", &["png", "jpg", "jpeg", "gif"])
            .add_filter("Заводская анимация Flydigi", &["bin"])
            .pick_file();
        if let Some(p) = file {
            let _ = tx.send(load(p).map_err(|e| format!("{e:#}")));
        }
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
            app.screen.result = Some((format!("Не открылось: {e}"), true));
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
        Err(TryRecvError::Disconnected) => Err("установка прервалась".into()),
    };
    app.screen.installing = None;
    app.screen.rule = None;
    app.toast = Some(match r {
        Ok(()) => ("Правило udev установлено".into(), false, Instant::now()),
        Err(e) => (format!("Правило не установлено: {e}"), true, Instant::now()),
    });
}

/// "Install the rule" with a spinner while the system prompt is up.
/// Returns whether it was pressed.
fn rule_button(ui: &mut egui::Ui, busy: bool) -> bool {
    ui.horizontal(|ui| {
        let r = widgets::secondary(ui, "Установить правило…", !busy).on_hover_text(format!(
            "Запишет {} и перечитает правила udev. Пароль спросит система, не приложение",
            apex5::udev::PATH
        ));
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
            ("Картинка загружена".to_string(), false)
        }
        Err(e) if started => (
            format!(
                "Не загружено: {e}. Попробуйте ещё раз: экран ждёт загрузку, пока геймпад \
                 не выключат. Если не выходит, выключите и включите геймпад переключателем сзади"
            ),
            true,
        ),
        Err(e) => (format!("Не загружено: {e}"), true),
    };
    app.toast = Some((msg.clone(), err, Instant::now()));
    app.screen.result = Some((msg, err));
}

/// What a change in the card asks for, applied once the draft is let go.
enum Action {
    Pick,
    InstallRule,
    Upload(Upload),
    Clear,
}

/// The upload part of the settings page's "Экран" card.
pub fn card(app: &mut App, ui: &mut egui::Ui) {
    poll(app);
    poll_install(app);
    let mut action = None;
    let loading = app.screen.loading.is_some();
    row(
        ui,
        "Своя картинка",
        "PNG, JPEG или GIF до 150 кадров, экран 160 × 80. Заводскую анимацию возвращает файл \
         default_screen_image_<тип>.bin из Flydigi Space Station",
        |ui| {
            if loading {
                ui.add(egui::Spinner::new().color(accent()));
            }
            if widgets::secondary(ui, "Выбрать файл…", !loading && !app.busy).clicked()
            {
                action = Some(Action::Pick);
            }
        },
    );
    if let Some((msg, err)) = &app.screen.result {
        ui.label(RichText::new(msg).color(if *err { BAD } else { OK }));
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
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            preview(ui, d);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(&d.name).color(TEXT));
                let n = d.upload.frames;
                let what = match (&d.source, n) {
                    (None, _) => format!("Заводская анимация Flydigi, кадров: {n}"),
                    (Some(_), 1) => "Картинка".to_string(),
                    (Some(_), _) => format!("Анимация, кадров: {n}"),
                };
                hint(ui, &what);
                if d.source.as_ref().is_some_and(|s| s.cut) {
                    ui.label(
                        RichText::new(format!(
                            "В GIF больше {} кадров, взяты первые {}",
                            screen::MAX_FRAMES,
                            screen::MAX_FRAMES
                        ))
                        .size(12.5)
                        .color(WARN),
                    );
                }
                ui.add_space(6.0);
                let mut changed = false;
                if d.source.is_some() {
                    changed |= widgets::segmented(
                        ui,
                        &mut d.fit,
                        &[(Fit::Fill, "Заполнить экран"), (Fit::Whole, "Целиком")],
                    );
                }
                if d.source.is_some() && n > 1 {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Кадр").color(DIM));
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut d.interval_ms)
                                    .range(screen::INTERVAL_MS.0..=screen::INTERVAL_MS.1)
                                    .speed(2.0)
                                    .suffix(" мс"),
                            )
                            .changed();
                    });
                }
                if changed && let Some(src) = &d.source {
                    match encode(src, d.fit, d.interval_ms) {
                        Ok(u) => {
                            d.upload = u;
                            d.textures.clear();
                        }
                        Err(e) => {
                            ui.label(RichText::new(format!("{e:#}")).color(BAD));
                        }
                    }
                }
                ui.add_space(6.0);
                let minutes = (n as u32 * screen::SECONDS_PER_FRAME).div_ceil(60);
                hint(ui, &format!("Загрузка займёт около {minutes} мин"));
            });
        });

        ui.add_space(10.0);
        let wrong_model = d.source.is_none() && d.factory_type != device_type;
        let blocker = if !wired {
            Some("Подключите геймпад кабелем: через донгл загрузка невозможна".to_string())
        } else if !rule {
            Some(screen::RULE_HINT.to_string())
        } else if wrong_model {
            Some(match device_type {
                Some(t) => {
                    format!("Нужен файл default_screen_image_{t}.bin: этот для другой модели")
                }
                None => "Геймпад не подключён".to_string(),
            })
        } else {
            None
        };
        match &blocker {
            Some(b) => {
                ui.label(RichText::new(b).color(WARN));
                if wired && !rule {
                    ui.add_space(4.0);
                    if rule_button(ui, installing) {
                        action = Some(Action::InstallRule);
                    }
                }
            }
            None => hint(
                ui,
                "Пока идёт загрузка, не отключайте кабель и не выключайте геймпад. \
                 Приложением в это время пользоваться нельзя. В конце геймпад перезагрузится.",
            ),
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let text = if armed {
                "Точно загрузить? Нажмите ещё раз"
            } else {
                "Загрузить на геймпад"
            };
            if widgets::primary(ui, text, blocker.is_none() && !busy).clicked() {
                if armed {
                    arm = Some(None);
                    action = Some(Action::Upload(d.upload.clone()));
                } else {
                    arm = Some(Some(Instant::now()));
                }
            }
            if armed {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
                if ui.button("Отмена").clicked() {
                    arm = Some(None);
                }
            } else if widgets::secondary(ui, "Убрать", true).clicked() {
                action = Some(Action::Clear);
            }
        });
    }

    if let Some(a) = arm {
        app.screen.armed = a;
    }
    match action {
        Some(Action::Pick) => pick(app, ui.ctx()),
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

/// The frames as the screen will show them, at twice the size.
fn preview(ui: &mut egui::Ui, d: &mut Draft) {
    if d.textures.len() != d.upload.frames {
        d.textures = (0..d.upload.frames)
            .map(|i| {
                let rgba = screen::to_rgba(d.upload.frame(i));
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [screen::W as usize, screen::H as usize],
                    rgba.as_raw(),
                );
                ui.ctx()
                    .load_texture(format!("screen-{i}"), img, egui::TextureOptions::NEAREST)
            })
            .collect();
    }
    let n = d.textures.len();
    let i = if n > 1 {
        let ms = (ui.input(|i| i.time) * 1000.0) as u64;
        ui.ctx()
            .request_repaint_after(Duration::from_millis(d.interval_ms as u64));
        (ms / d.interval_ms.max(10) as u64) as usize % n
    } else {
        0
    };
    let size = vec2(screen::W as f32 * 2.0, screen::H as f32 * 2.0);
    egui::Frame::new()
        .fill(egui::Color32::BLACK)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.add(
                egui::Image::new(&d.textures[i])
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
            RichText::new("Загрузка картинки на экран")
                .font(theme::bold(24.0))
                .color(TEXT),
        );
        ui.add_space(16.0);
        match &job.stage {
            None => {
                ui.add(egui::Spinner::new().size(22.0).color(accent()));
                ui.label(RichText::new("Проверка подключения…").color(DIM));
            }
            Some(Stage::Switching) => {
                ui.add(egui::Spinner::new().size(22.0).color(accent()));
                ui.label(RichText::new("Экран переходит в режим загрузки…").color(DIM));
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
                    format!(", осталось около {}", clock(rest))
                } else {
                    String::new()
                };
                ui.label(
                    RichText::new(format!(
                        "{:.0} % · прошло {}{left}",
                        f * 100.0,
                        clock(spent)
                    ))
                    .color(DIM),
                );
            }
            Some(Stage::Saving) => {
                ui.add(egui::Spinner::new().size(22.0).color(accent()));
                ui.label(
                    RichText::new(
                        "Экран сохраняет картинку, затем геймпад перезагрузится. Около 20 секунд.",
                    )
                    .color(DIM),
                );
            }
        }
        ui.add_space(20.0);
        ui.label(
            RichText::new(
                "Не отключайте кабель, не выключайте геймпад и не закрывайте приложение.",
            )
            .color(WARN),
        );
        ui.label(
            RichText::new("Прерванная загрузка оставит экран без картинки, но её можно повторить.")
                .size(12.5)
                .color(FAINT),
        );
    });
}
