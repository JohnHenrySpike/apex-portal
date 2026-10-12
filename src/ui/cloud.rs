//! What the app takes from Flydigi's server, always on the user's request:
//! firmware news on the settings page, the official screen pictures, the
//! games' trigger vibration presets.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use apex5::cloud::{self, FirmwareNews, GamePreset, Picture};
use apex5::device::Info;
use apex5::profile::Profile;
use apex5::screen::{self, Fit};
use eframe::egui::{self, CornerRadius, RichText, Stroke, TextureHandle, vec2};

use super::theme::{BAD, BORDER, CARD_HI, DIM, OK, TEXT, WARN, accent};
use super::widgets::{self, card_titled, hint, row};

/// A request running off the UI thread and what it brought back.
pub struct Task<T> {
    rx: Option<Receiver<Result<T, String>>>,
    pub value: Option<T>,
    pub error: Option<String>,
}

impl<T> Default for Task<T> {
    fn default() -> Self {
        Task {
            rx: None,
            value: None,
            error: None,
        }
    }
}

impl<T: Send + 'static> Task<T> {
    fn start(
        &mut self,
        ctx: &egui::Context,
        f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f().map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        self.rx = Some(rx);
        self.error = None;
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.rx = None;
                match r {
                    Ok(v) => self.value = Some(v),
                    Err(e) => self.error = Some(e),
                }
            }
            Err(TryRecvError::Disconnected) => self.rx = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }
}

#[derive(Default)]
pub struct State {
    firmware: Task<Vec<FirmwareNews>>,
    /// The pad the firmware answer is for.
    firmware_uid: String,
    games: Task<Vec<GamePreset>>,
    game_filter: String,
    game_selected: Option<i64>,
    /// The last preset put into the edited profile.
    pub game_applied: Option<String>,
    pictures: Task<Vec<Picture>>,
    pub gallery_open: bool,
    thumbs: HashMap<i64, TextureHandle>,
    thumb_rx: Option<Receiver<(i64, Option<egui::ColorImage>)>>,
    /// Pictures whose thumbnail did not come.
    thumbs_failed: Vec<i64>,
}

fn spinner(ui: &mut egui::Ui) {
    ui.add(egui::Spinner::new().color(accent()));
}

fn error(ui: &mut egui::Ui, e: &str) {
    ui.label(RichText::new(e).color(BAD));
}

// ------------------------------------------------------------------ firmware

fn chip_name(chip: &str) -> &str {
    match chip {
        "main" => "основная",
        "switch" => "Switch",
        "screen" => "экран",
        "NearLink" => "радио NearLink",
        "dongle" => "донгл",
        "trigger" => "курки",
        other => other,
    }
}

/// The firmware check under the version list on the settings page.
pub fn firmware(st: &mut State, info: &Info, ui: &mut egui::Ui) {
    st.firmware.poll();
    if st.firmware_uid != info.uid {
        st.firmware = Task::default();
    }
    let busy = st.firmware.busy();
    let mut check = false;
    row(
        ui,
        "Обновления прошивки",
        "Сверить версии с сервером Flydigi. Туда уходят только модель и версии прошивок",
        |ui| {
            if busy {
                spinner(ui);
            }
            check = widgets::secondary(ui, "Проверить", !busy).clicked();
        },
    );
    if check {
        let info = info.clone();
        st.firmware_uid = info.uid.clone();
        st.firmware.start(ui.ctx(), move || cloud::firmware(&info));
    }
    if let Some(e) = &st.firmware.error {
        error(ui, e);
    }
    let Some(news) = &st.firmware.value else {
        return;
    };
    if news.is_empty() {
        ui.label(RichText::new("Все прошивки последние").color(OK));
        return;
    }
    for n in news {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Прошивка {}: есть {}, у геймпада {}",
                chip_name(n.chip),
                n.latest,
                n.current
            ))
            .color(WARN),
        );
        if !n.notes.is_empty() {
            ui.label(RichText::new(&n.notes).size(12.5).color(DIM));
        }
    }
    ui.add_space(4.0);
    hint(
        ui,
        "Обновить прошивку можно только во Flydigi Space Station на Windows: \
         это приложение геймпад не прошивает. Описание изменений — от Flydigi.",
    );
}

// ------------------------------------------------------------------ game presets

/// Flydigi's per-game trigger vibration, put into both triggers of the
/// edited profile.
pub fn game_presets(st: &mut State, p: &mut Profile, ui: &mut egui::Ui) {
    st.games.poll();
    card_titled(ui, "Пресеты курков для игр", |ui| {
        hint(
            ui,
            "Flydigi подбирает для игр, как вибрация рукояток передаётся в курки. Пресет ставит \
             оба курка в режим «Вибрация игры» с настройками этой игры. Список берётся с сервера Flydigi.",
        );
        ui.add_space(6.0);
        let busy = st.games.busy();
        let Some(games) = &st.games.value else {
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "Загрузить список игр", !busy).clicked()
                {
                    st.games.start(ui.ctx(), cloud::game_presets);
                }
                if busy {
                    spinner(ui);
                }
            });
            if let Some(e) = &st.games.error {
                error(ui, e);
            }
            return;
        };

        ui.add(
            egui::TextEdit::singleline(&mut st.game_filter)
                .hint_text(format!("Поиск среди {} игр", games.len()))
                .desired_width(f32::INFINITY)
                .margin(vec2(10.0, 6.0)),
        );
        ui.add_space(6.0);
        let filter = st.game_filter.to_lowercase();
        let mut apply = None;
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for g in games
                    .iter()
                    .filter(|g| g.name.to_lowercase().contains(&filter))
                {
                    let sel = st.game_selected == Some(g.id);
                    let b = &g.bind[0];
                    // the selection's own text colour reads on its fill
                    let (name, detail) = (
                        RichText::new(&g.name),
                        RichText::new(format!("порог {} · сила {}", b.block, b.scale)).size(12.0),
                    );
                    let (name, detail) = if sel {
                        (name, detail)
                    } else {
                        (name.color(TEXT), detail.color(DIM))
                    };
                    let r = ui.add(
                        egui::Button::selectable(sel, name)
                            .right_text(detail)
                            .min_size(vec2(ui.available_width(), 28.0)),
                    );
                    if r.clicked() {
                        st.game_selected = Some(g.id);
                    }
                    if r.double_clicked() {
                        apply = Some(g.clone());
                    }
                }
            });
        ui.add_space(8.0);
        let chosen = games.iter().find(|g| Some(g.id) == st.game_selected);
        ui.horizontal(|ui| {
            if widgets::primary(ui, "Поставить в оба курка", chosen.is_some()).clicked()
            {
                apply = chosen.cloned();
            }
            if let Some(g) = chosen {
                let [l, r] = &g.bind;
                let detail = |b: &apex5::profile::TriggerBind| {
                    format!("ход {}, частота {}", b.params[0], b.params[3])
                };
                let text = if l == r {
                    detail(l)
                } else {
                    format!("слева {}; справа {}", detail(l), detail(r))
                };
                ui.label(RichText::new(text).size(12.5).color(DIM));
            }
        });
        if let Some(g) = apply {
            p.set_vibration_preset(0, &g.bind[0]);
            p.set_vibration_preset(1, &g.bind[1]);
            st.game_applied = Some(g.name);
        }
        if let Some(name) = &st.game_applied {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Поставлен пресет «{name}». Он начнёт работать после «Применить»; \
                     вибрацию рукояток в профиле тоже нужно включить."
                ))
                .color(OK),
            );
        }
    });
}

// ------------------------------------------------------------------ screen pictures

/// Downloads and renders the thumbnails as the screen shows them, a few at
/// a time; the order on screen does not depend on which arrives first.
fn load_thumbs(st: &mut State, ctx: &egui::Context, pics: Vec<Picture>) {
    const THREADS: usize = 6;
    let (tx, rx) = channel();
    // popped from the end: the first on screen go first
    let queue = std::sync::Arc::new(std::sync::Mutex::new(
        pics.into_iter().rev().collect::<Vec<_>>(),
    ));
    for _ in 0..THREADS {
        let (tx, ctx, queue) = (tx.clone(), ctx.clone(), queue.clone());
        std::thread::spawn(move || {
            loop {
                let Some(p) = queue.lock().unwrap().pop() else {
                    return;
                };
                let img = p
                    .fetch()
                    .ok()
                    .and_then(|path| image::open(path).ok())
                    .map(|img| {
                        let frame = screen::encode(&img.to_rgba8(), Fit::Fill);
                        let rgba = screen::to_rgba(&frame);
                        egui::ColorImage::from_rgba_unmultiplied(
                            [screen::W as usize, screen::H as usize],
                            rgba.as_raw(),
                        )
                    });
                if tx.send((p.id, img)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }
    st.thumb_rx = Some(rx);
}

fn poll_thumbs(st: &mut State, ctx: &egui::Context) {
    let Some(rx) = &st.thumb_rx else { return };
    loop {
        match rx.try_recv() {
            Ok((id, Some(img))) => {
                let t = ctx.load_texture(
                    format!("flydigi-pic-{id}"),
                    img,
                    egui::TextureOptions::NEAREST,
                );
                st.thumbs.insert(id, t);
            }
            Ok((id, None)) => st.thumbs_failed.push(id),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                st.thumb_rx = None;
                break;
            }
        }
    }
}

/// Flydigi's official pictures for the screen; returns the one clicked.
pub fn gallery(st: &mut State, enabled: bool, ui: &mut egui::Ui) -> Option<Picture> {
    st.pictures.poll();
    poll_thumbs(st, ui.ctx());
    if st.pictures.value.is_none() && !st.pictures.busy() && st.pictures.error.is_none() {
        st.pictures.start(ui.ctx(), cloud::pictures);
    }
    if let Some(e) = &st.pictures.error {
        error(ui, e);
        if widgets::secondary(ui, "Повторить", true).clicked() {
            st.pictures.error = None;
        }
        return None;
    }
    let Some(pics) = &st.pictures.value else {
        spinner(ui);
        return None;
    };
    if st.thumb_rx.is_none() && st.thumbs.is_empty() && st.thumbs_failed.is_empty() {
        let pics = pics.clone();
        load_thumbs(st, ui.ctx(), pics);
    }
    let pics = st.pictures.value.as_ref().unwrap();
    let left = pics.len() - st.thumbs.len() - st.thumbs_failed.len();
    if left > 0 {
        hint(ui, &format!("Загружаются картинки: осталось {left}"));
    } else if !st.thumbs_failed.is_empty() {
        let mut retry = false;
        ui.horizontal(|ui| {
            error(ui, &format!("Не скачались: {}", st.thumbs_failed.len()));
            retry = widgets::secondary(ui, "Повторить", true).clicked();
        });
        if retry {
            let again: Vec<Picture> = pics
                .iter()
                .filter(|p| st.thumbs_failed.contains(&p.id))
                .cloned()
                .collect();
            st.thumbs_failed.clear();
            load_thumbs(st, ui.ctx(), again);
        }
    }
    let pics = st.pictures.value.as_ref().unwrap();
    ui.add_space(4.0);
    let mut picked = None;
    let size = vec2(screen::W as f32, screen::H as f32);
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for p in pics {
                    let r = match st.thumbs.get(&p.id) {
                        Some(t) => ui.add_enabled(
                            enabled,
                            egui::Button::image(
                                egui::Image::new(t)
                                    .fit_to_exact_size(size)
                                    .corner_radius(CornerRadius::same(4)),
                            )
                            .frame(false),
                        ),
                        None => {
                            let (rect, r) = ui.allocate_exact_size(size, egui::Sense::hover());
                            ui.painter().rect(
                                rect,
                                CornerRadius::same(4),
                                CARD_HI,
                                Stroke::new(1.0, BORDER),
                                egui::StrokeKind::Inside,
                            );
                            r
                        }
                    };
                    if r.on_hover_text(&p.title).clicked() {
                        picked = Some(p.clone());
                    }
                }
            });
        });
    picked
}
