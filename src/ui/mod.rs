//! Window layout, overview and device settings.

use std::time::{Duration, Instant};

use apex5::device::{self, NICKNAME_MAX};
use apex5::worker::Cmd;
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, pos2, vec2};

use crate::{App, Tab};

pub mod cloud;
pub mod copies;
mod dualsense;
mod input;
pub mod lighting;
mod pad;
mod profile;
pub mod screen;
pub mod theme;
mod widgets;

use theme::{BAD, BORDER, DIM, FAINT, OK, SIDEBAR, TEXT, WARN, accent};
use widgets::{Icon, card, card_titled, page_title, row, toggle_row};

const NAV: [(Tab, Icon, &str); 7] = [
    (Tab::Overview, Icon::Home, "Геймпад"),
    (Tab::Lighting, Icon::Light, "Подсветка"),
    (Tab::Screen, Icon::Screen, "Экран"),
    (Tab::Profile, Icon::Profiles, "Профили"),
    (Tab::DualSense, Icon::Waves, "Режим DualSense"),
    (Tab::Input, Icon::Input, "Тест ввода"),
    (Tab::Settings, Icon::Settings, "Настройки"),
];

/// Where a thin divider goes in the menu: after the pad's own looks, and
/// before the tools.
const NAV_BREAKS: [Tab; 2] = [Tab::Profile, Tab::Input];

pub fn shell(app: &mut App, ui: &mut egui::Ui) {
    window_edges(ui.ctx());
    if app.screen.job.is_some() {
        title_bar(app, ui, false);
        toast(app, ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ui, |ui| screen::job_page(app, ui));
        return;
    }
    sidebar(app, ui);
    title_bar(app, ui, app.info.is_some());
    if app.info.is_some() {
        match app.tab {
            Tab::Profile => profile::action_bar(app, ui),
            Tab::Lighting => lighting::action_bar(app, ui),
            _ => {}
        }
    }
    toast(app, ui);
    copies::dialogs(app, ui);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin {
                    left: 28,
                    right: 18,
                    top: 0,
                    bottom: 0,
                }),
        )
        .show(ui, |ui| {
            if app.info.is_none() {
                disconnected(app, ui);
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // a column of at most 1100 px, centred in wide windows
                    let room = ui.available_width() - 14.0;
                    let w = room.min(1100.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(((room - w) / 2.0).floor());
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            page(app, ui);
                        });
                    });
                    ui.add_space(24.0);
                });
        });
}

fn page(app: &mut App, ui: &mut egui::Ui) {
    match app.tab {
        Tab::Overview => overview(app, ui),
        Tab::Lighting => lighting::show(app, ui),
        Tab::Screen => screen::page(app, ui),
        Tab::Profile => profile::show(app, ui),
        Tab::DualSense => dualsense::page(app, ui),
        Tab::Input => input::show(app, ui),
        Tab::Settings => settings(app, ui),
    }
}

// ------------------------------------------------------------------ window

/// Moves the window when dragged; a double click maximizes it.
fn drag_area(ui: &mut egui::Ui, rect: egui::Rect, id: &str) {
    let r = ui.interact(rect, egui::Id::new(id), egui::Sense::click_and_drag());
    if r.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if r.double_clicked() {
        let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
    }
}

/// The strip above the page: drag to move, refresh, and the window buttons.
fn title_bar(app: &App, ui: &mut egui::Ui, refresh: bool) {
    egui::Panel::top("title_bar")
        .exact_size(44.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin {
                    left: 28,
                    right: 6,
                    top: 6,
                    bottom: 0,
                }),
        )
        .show(ui, |ui| {
            drag_area(ui, ui.max_rect(), "title_drag");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                if window_button(ui, Icon::Close, "Закрыть", true).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let (icon, tip) = if max {
                    (Icon::Restore, "Восстановить")
                } else {
                    (Icon::Maximize, "Развернуть")
                };
                if window_button(ui, icon, tip, false).clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
                if window_button(ui, Icon::Minimize, "Свернуть", false).clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                ui.add_space(14.0);
                if refresh {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if widgets::icon_button(ui, Icon::Refresh, "Перечитать всё с геймпада")
                        .clicked()
                    {
                        app.send(Cmd::Refresh);
                    }
                    if app.busy {
                        ui.add(egui::Spinner::new().size(18.0).color(accent()));
                    }
                }
            });
        });
}

fn window_button(ui: &mut egui::Ui, which: Icon, tip: &str, close: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(40.0, 30.0), egui::Sense::click());
    let (bg, fg) = match (r.hovered(), close) {
        (true, true) => (Color32::from_rgb(196, 43, 28), Color32::WHITE),
        (true, false) => (theme::HOVER, TEXT),
        _ => (Color32::TRANSPARENT, DIM),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    widgets::icon(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), vec2(11.0, 11.0)),
        which,
        fg,
    );
    r.on_hover_text(tip)
}

/// Without the system frame the window's edges resize it, and a hairline
/// marks them.
fn window_edges(ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let (max, full) = ctx.input(|i| {
        let v = i.viewport();
        (
            v.maximized.unwrap_or(false) || v.fullscreen.unwrap_or(false),
            i.content_rect(),
        )
    });
    if max {
        return;
    }
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("window_edge"),
    ))
    .rect_stroke(
        full,
        0.0,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );

    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    const EDGE: f32 = 5.0;
    const CORNER: f32 = 14.0;
    let (l, r) = (pos.x - full.left() < EDGE, full.right() - pos.x < EDGE);
    let (t, b) = (pos.y - full.top() < EDGE, full.bottom() - pos.y < EDGE);
    // corners are easier to catch a little further along the edge
    let (lc, rc) = (pos.x - full.left() < CORNER, full.right() - pos.x < CORNER);
    let (tc, bc) = (pos.y - full.top() < CORNER, full.bottom() - pos.y < CORNER);
    let dir = if (t && lc) || (l && tc) {
        Some((D::NorthWest, C::ResizeNorthWest))
    } else if (t && rc) || (r && tc) {
        Some((D::NorthEast, C::ResizeNorthEast))
    } else if (b && lc) || (l && bc) {
        Some((D::SouthWest, C::ResizeSouthWest))
    } else if (b && rc) || (r && bc) {
        Some((D::SouthEast, C::ResizeSouthEast))
    } else if l {
        Some((D::West, C::ResizeWest))
    } else if r {
        Some((D::East, C::ResizeEast))
    } else if t {
        Some((D::North, C::ResizeNorth))
    } else if b {
        Some((D::South, C::ResizeSouth))
    } else {
        None
    };
    if let Some((d, cursor)) = dir {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d));
        }
    }
}

// ------------------------------------------------------------------ sidebar

fn sidebar(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("nav")
        .resizable(false)
        .exact_size(232.0)
        .frame(
            egui::Frame::new()
                .fill(SIDEBAR)
                .inner_margin(egui::Margin::symmetric(16, 20)),
        )
        .show(ui, |ui| {
            // The wordmark moves the window like a title bar.
            let top =
                egui::Rect::from_min_size(ui.max_rect().min - vec2(16.0, 20.0), vec2(232.0, 76.0));
            drag_area(ui, top, "nav_drag");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new("APEX")
                        .font(theme::bold(24.0))
                        .color(theme::SILVER),
                );
                ui.label(RichText::new("5").font(theme::bold(24.0)).color(accent()));
            });
            ui.label(
                RichText::new("C O N T R O L")
                    .font(theme::medium(10.5))
                    .color(FAINT),
            );
            ui.add_space(16.0);

            device_badge(app, ui);
            ui.add_space(14.0);

            for (tab, icon, label) in NAV {
                if NAV_BREAKS.contains(&tab) {
                    ui.add_space(3.0);
                    widgets::separator(ui);
                    ui.add_space(5.0);
                }
                nav_item(app, ui, tab, icon, label);
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    widgets::icon(ui.painter(), rect, Icon::Shield, FAINT);
                    ui.label(RichText::new("Защищённый режим").size(12.0).color(FAINT));
                })
                .response
                .on_hover_text(
                    "Опасные команды заблокированы: приложение не отправляет команды прошивки, \
                     сброса к заводским, смены режима и типа устройства. Каждый пакет \
                     проверяется по белому списку.",
                );
            });
        });
}

/// The pad's state and its four profiles, on every page.
fn device_badge(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            let Some(i) = app.info.clone() else {
                ui.horizontal(|ui| {
                    status_dot(ui, BAD);
                    ui.label(
                        RichText::new("Не подключён")
                            .font(theme::medium(14.0))
                            .color(TEXT),
                    );
                });
                ui.label(RichText::new("ожидание геймпада…").size(12.5).color(DIM));
                return;
            };
            ui.horizontal(|ui| {
                status_dot(ui, OK);
                ui.label(
                    RichText::new(format!("Flydigi {}", i.model()))
                        .font(theme::medium(14.0))
                        .color(TEXT),
                );
            });
            ui.horizontal(|ui| {
                widgets::battery(ui, i.battery_level, i.charging);
                let b = format!("{}%", i.battery_level as u32 * 20);
                ui.label(RichText::new(b).size(12.5).color(DIM));
                ui.label(RichText::new("·").color(FAINT));
                ui.label(
                    RichText::new(if i.wired {
                        "кабель"
                    } else {
                        "донгл"
                    })
                    .size(12.5)
                    .color(DIM),
                );
            });
            ui.add_space(6.0);
            profile_switch(app, ui);
            if let Some(p) = &app.pad_profile {
                let t = profile::shown_title(&p.title());
                ui.add(
                    egui::Label::new(
                        RichText::new(if t.is_empty() {
                            "без названия".into()
                        } else {
                            t
                        })
                        .size(12.5)
                        .color(DIM),
                    )
                    .truncate(),
                );
            }
        });
}

/// Four buttons for the pad's profile slots.
fn profile_switch(app: &mut App, ui: &mut egui::Ui) {
    let gap = 4.0;
    let w = ((ui.available_width() - 3.0 * gap) / 4.0).floor();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for slot in 0..apex5::profile::SLOTS {
            let active = slot == app.active;
            let (rect, r) = ui.allocate_exact_size(vec2(w, 30.0), egui::Sense::click());
            let t = ui.ctx().animate_bool_responsive(r.id, active);
            let bg = if r.hovered() && !active {
                theme::HOVER
            } else {
                theme::CARD_HI
            };
            ui.painter().rect(
                rect,
                CornerRadius::same(8),
                widgets::lerp_colour(bg, accent(), t),
                Stroke::new(1.0, widgets::lerp_colour(BORDER, accent(), t)),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("P{}", slot + 1),
                theme::medium(13.5),
                if active { theme::on_accent() } else { DIM },
            );
            let r = r.on_hover_text(if active {
                "Активный профиль"
            } else {
                "Переключить геймпад на этот профиль"
            });
            if r.clicked() && !active && !app.busy {
                switch_profile(app, slot);
            }
        }
    });
}

fn status_dot(ui: &mut egui::Ui, c: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 6.0, c.gamma_multiply(0.2));
    ui.painter().circle_filled(rect.center(), 3.5, c);
}

fn nav_item(app: &mut App, ui: &mut egui::Ui, tab: Tab, icon: Icon, label: &str) {
    let sel = app.tab == tab;
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), egui::Sense::click());
    let t = ui.ctx().animate_bool_responsive(r.id, sel);
    let p = ui.painter();
    if sel || r.hovered() {
        let bg = if sel { theme::CARD_HI } else { theme::CARD };
        p.rect_filled(rect, CornerRadius::same(10), bg);
    }
    if t > 0.0 {
        let bar = egui::Rect::from_min_size(
            pos2(rect.left(), rect.center().y - 10.0 * t),
            vec2(3.0, 20.0 * t),
        );
        p.rect_filled(bar, CornerRadius::same(2), accent());
    }
    let colour = if sel {
        accent()
    } else if r.hovered() {
        TEXT
    } else {
        DIM
    };
    let ir =
        egui::Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(18.0, 18.0));
    widgets::icon(p, ir, icon, colour);
    p.text(
        pos2(rect.left() + 48.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::medium(14.5),
        if sel { TEXT } else { colour },
    );
    if tab == Tab::Profile && (app.profile_dirty() || app.flash_dirty) {
        p.circle_filled(pos2(rect.right() - 16.0, rect.center().y), 4.0, WARN);
        r.clone()
            .on_hover_text("Есть несохранённые изменения профиля");
    }
    if r.clicked() {
        app.tab = tab;
    }
    ui.add_space(2.0);
}

/// Page header: the title and a line under it.
pub fn header(_app: &App, ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        page_title(ui, title, subtitle);
    });
    ui.add_space(14.0);
}

fn toast(app: &App, ui: &mut egui::Ui) {
    let Some((msg, err, t)) = &app.toast else {
        return;
    };
    if t.elapsed() > Duration::from_secs(5) {
        return;
    }
    let colour = if *err { BAD } else { OK };
    egui::Area::new(egui::Id::new("toast"))
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            // above the profile's Apply/Save bar
            vec2(
                -24.0,
                if app.tab == Tab::Profile {
                    -84.0
                } else {
                    -24.0
                },
            ),
        )
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(theme::CARD_HI)
                .stroke(Stroke::new(1.0, colour.gamma_multiply(0.6)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(16, 12))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(150),
                })
                .show(ui, |ui| {
                    ui.set_max_width(380.0);
                    ui.horizontal(|ui| {
                        status_dot(ui, colour);
                        ui.label(RichText::new(msg).color(TEXT));
                    });
                });
        });
    ui.ctx().request_repaint_after(Duration::from_millis(250));
}

fn disconnected(app: &mut App, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        let w = ui.available_width().min(560.0);
        pad::show(
            ui,
            vec2(w, w * 0.62),
            &pad::Opts {
                brightness: 0.0,
                ..Default::default()
            },
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Геймпад не найден")
                .font(theme::bold(24.0))
                .color(TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Подключите Apex 5 кабелем или через донгл — приложение найдёт его само.",
            )
            .color(DIM),
        );
        ui.label(RichText::new(&app.status).size(12.5).color(FAINT));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Если геймпад подключён, но нет доступа, установите правило udev.")
                .size(12.5)
                .color(FAINT),
        );
        if app.status.contains("udev") {
            ui.add_space(8.0);
            ui.scope(|ui| {
                ui.set_max_width(260.0);
                screen::rule_offer(app, ui);
            });
        }
        ui.add_space(12.0);
        ui.add(egui::Spinner::new().size(22.0).color(accent()));
    });
}

// ------------------------------------------------------------------ overview

fn overview(app: &mut App, ui: &mut egui::Ui) {
    let Some(info) = app.info.clone() else { return };
    header(
        app,
        ui,
        &format!("Flydigi {}", info.model()),
        "Состояние геймпада и быстрые действия",
    );

    let wide = ui.available_width() > 860.0;
    let hero = |app: &mut App, ui: &mut egui::Ui| {
        card(ui, |ui| {
            let w = ui.available_width();
            let leds = app
                .led
                .as_ref()
                .map(|l| lighting::current_frame(&l.cfg, ui.input(|i| i.time)));
            let bright = app
                .led
                .as_ref()
                .map(|l| l.cfg.brightness() as f32 / 100.0)
                .unwrap_or(0.0);
            let screen = format!("P{}", app.active + 1);
            pad::show(
                ui,
                vec2(w, (w * 0.66).min(440.0)),
                &pad::Opts {
                    leds: leds.as_deref(),
                    brightness: bright,
                    screen: Some(&screen),
                    ..Default::default()
                },
            );
            if app.led.as_ref().is_some_and(|l| l.effect.animated()) {
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        });
    };

    if wide {
        widgets::split(ui, 0.6, app, hero, status_cards);
    } else {
        hero(app, ui);
        ui.add_space(12.0);
        status_cards(app, ui);
    }

    ui.add_space(12.0);
    about(app, ui, &info);
}

/// Model, firmware, the stored name and Flydigi's firmware news.
fn about(app: &mut App, ui: &mut egui::Ui, info: &device::Info) {
    card_titled(ui, "Об устройстве", |ui| {
        egui::Grid::new("about")
            .num_columns(2)
            .spacing([28.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Модель").color(DIM));
                ui.label(format!(
                    "Flydigi {} (тип {})",
                    info.model(),
                    info.device_type
                ));
                ui.end_row();
                ui.label(RichText::new("UID").color(DIM));
                ui.monospace(&info.uid);
                ui.end_row();
                for (name, v) in &info.firmware {
                    ui.label(RichText::new(format!("Прошивка: {name}")).color(DIM));
                    ui.monospace(v);
                    ui.end_row();
                }
                ui.label(RichText::new("Имя").color(DIM)).on_hover_text(
                    "Служебное имя, которое геймпад хранит в себе. Его показывают Flydigi Space \
                     Station и другие программы, чтобы различать несколько геймпадов. На экране \
                     геймпада и в играх оно не отображается.",
                );
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut app.nickname)
                            .desired_width(200.0)
                            .hint_text("без имени")
                            .margin(vec2(10.0, 5.0)),
                    );
                    let n = app.nickname.len();
                    ui.label(
                        RichText::new(format!("{n}/{NICKNAME_MAX}"))
                            .size(12.0)
                            .color(FAINT),
                    );
                    let ok = n > 0 && n <= NICKNAME_MAX && app.nickname != info.nickname;
                    if ok && widgets::secondary(ui, "Сохранить", true).clicked() {
                        app.send(Cmd::SetNickname(app.nickname.clone()));
                    }
                });
                ui.end_row();
            });
        ui.add_space(4.0);
        widgets::separator(ui);
        ui.add_space(4.0);
        cloud::firmware(&mut app.cloud, info, ui);
    });
}

pub fn switch_profile(app: &mut App, slot: u8) {
    if app.profile_dirty() {
        app.toast = Some((
            "Сначала примените или отмените изменения профиля".into(),
            true,
            std::time::Instant::now(),
        ));
    } else {
        app.send(Cmd::SwitchProfile(slot));
    }
}

fn status_cards(app: &mut App, ui: &mut egui::Ui) {
    if let Some(p) = app.pad_profile.clone() {
        let title = p.title();
        // a factory title only repeats the slot
        let caption = if title.is_empty() || profile::shown_title(&title) != title {
            format!("Профиль P{}", app.active + 1)
        } else {
            format!("Профиль P{} · {title}", app.active + 1)
        };
        card_titled(ui, &caption, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (tab, name, value, notable) in profile::summary(&p) {
                let colour = if notable { accent() } else { TEXT };
                if widgets::link_row(ui, name, &value, colour).clicked() {
                    app.tab = Tab::Profile;
                    app.profile_tab = tab;
                }
            }
        });
        ui.add_space(12.0);
    }
    card_titled(ui, "Подсветка", |ui| {
        if let Some(led) = app.led.as_mut() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(lighting::effect_name(led.effect))
                        .font(theme::medium(14.5))
                        .color(TEXT),
                );
                for c in led.colours.iter().take(5) {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    ui.painter().circle(
                        rect.center(),
                        7.0,
                        Color32::from_rgb(c[0], c[1], c[2]),
                        Stroke::new(1.0, BORDER),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Яркость").color(DIM));
                let mut b = led.cfg.brightness();
                ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(80.0);
                if ui
                    .add(widgets::slider(&mut b, 0..=100).suffix("%"))
                    .changed()
                {
                    led.cfg.set_brightness(b);
                    led.dirty_since = Some(std::time::Instant::now());
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.link("Настроить подсветку →").clicked() {
                app.tab = Tab::Lighting;
            }
            if app.led_dirty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::secondary(ui, "Сохранить", !app.busy)
                        .on_hover_text("Без сохранения подсветка сбросится, когда геймпад уснёт")
                        .clicked()
                    {
                        app.save_led();
                    }
                    ui.label(RichText::new("не сохранена").size(12.5).color(WARN));
                });
            }
        });
    });
}

// ------------------------------------------------------------------ settings

/// Switches of the pad: feature number, name, hint.
type Feature = (u8, &'static str, &'static str);

const BUTTON_FEATURES: [Feature; 2] = [
    (
        1,
        "Быстрая смена профиля",
        "Fn + A/B/X/Y переключает профиль прямо с геймпада",
    ),
    (
        4,
        "Кнопка Turbo",
        "Turbo + кнопка включает турбо на геймпаде. Турбо из профиля работает и без неё",
    ),
];

const STICK_FEATURES: [Feature; 3] = [
    (
        5,
        "Фильтр дребезга",
        "Сглаживает дрожание; без него не работает автокалибровка",
    ),
    (
        6,
        "Автокалибровка",
        "Подстраивает центр стиков во время игры",
    ),
    (
        7,
        "Фильтр отскока",
        "Гасит обратный выброс при резком отпускании стика",
    ),
];

/// The supported switches of a list, with dividers between them.
fn features(app: &App, ui: &mut egui::Ui, s: &device::Settings, list: &[Feature]) {
    let mut first = true;
    for &(sub, name, hint_text) in list {
        if !s.is_supported(sub) {
            continue;
        }
        if !first {
            widgets::separator(ui);
        }
        first = false;
        let mut on = s.is_on(sub);
        let enabled = !(sub == 6 && !s.is_on(5));
        ui.add_enabled_ui(enabled, |ui| {
            if toggle_row(ui, name, hint_text, &mut on) {
                app.send(Cmd::SetFeature(sub, on));
            }
        });
    }
}

fn settings(app: &mut App, ui: &mut egui::Ui) {
    header(
        app,
        ui,
        "Настройки",
        "Применяются сразу и хранятся в самом геймпаде",
    );
    let Some(s) = app.settings.clone() else {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    };

    card_titled(ui, "Кнопки", |ui| {
        features(app, ui, &s, &BUTTON_FEATURES)
    });

    ui.add_space(12.0);
    card_titled(ui, "Стики", |ui| {
        features(app, ui, &s, &STICK_FEATURES);
        widgets::separator(ui);
        row(
            ui,
            "Точность",
            "Разрешение обычного (Xbox) ввода. Flydigi предлагает 8–12 бит",
            |ui| {
                combo(ui, "prec", s.precision, &device::PRECISION, |v| {
                    app.send(Cmd::SetNumber(21, v))
                });
            },
        );
        widgets::separator(ui);
        row(
            ui,
            "Чувствительность центра",
            "Насколько рано стик реагирует на отклонение",
            |ui| {
                combo(ui, "sens", s.sensitivity, &device::SENSITIVITY, |v| {
                    app.send(Cmd::SetNumber(22, v))
                });
            },
        );
    });

    ui.add_space(12.0);
    card_titled(ui, "Питание", |ui| {
        row(
            ui,
            "Автоотключение",
            "Через сколько минут бездействия геймпад засыпает",
            |ui| {
                // a value set elsewhere stays pickable alongside the presets
                let custom = format!("{} мин", s.sleep_minutes);
                let mut items = device::SLEEP.to_vec();
                if !items.iter().any(|(v, _)| *v == s.sleep_minutes) {
                    items.push((s.sleep_minutes, &custom));
                }
                combo(ui, "sleep", s.sleep_minutes, &items, |v| {
                    app.send(Cmd::SetNumber(23, v))
                });
            },
        );
    });

    let Some(info) = app.info.clone() else { return };
    if info.third_party_capable()
        && let Some(f) = app.flags.clone()
    {
        ui.add_space(12.0);
        card_titled(ui, "Сторонние программы", |ui| {
            let mut on = f.third_party;
            let r = ui.scope(|ui| {
                toggle_row(
                    ui,
                    "Разрешить перехват раскладки",
                    "Steam и другие программы на SDL смогут захватывать геймпад",
                    &mut on,
                )
            });
            r.response.on_hover_text(
                "Пока программа держит геймпад, профиль не действует, а обычный ввод (xpad) \
                 пропадает: игры видят геймпад только через эту программу.",
            );
            if r.inner {
                app.send(Cmd::SetThirdParty(on));
            }
            if !f.control_by.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Сейчас геймпад захвачен: {}. Профиль не действует, обычный ввод выключен.",
                        f.control_by
                    ))
                    .color(WARN),
                );
            }
        });
    }

    ui.add_space(12.0);
    reset_card(app, ui);
}

/// Factory profiles in all four slots, confirmed by a second press.
fn reset_card(app: &mut App, ui: &mut egui::Ui) {
    const WAIT: Duration = Duration::from_secs(5);
    card_titled(ui, "Сброс", |ui| {
        let armed = app.reset_armed.is_some_and(|t| t.elapsed() < WAIT);
        row(
            ui,
            "Все профили к заводским",
            "Кнопки, макросы, стики, курки, вибрация, гироскоп и названия всех четырёх профилей",
            |ui| {
                let text = if armed {
                    "Точно сбросить?"
                } else {
                    "Сбросить…"
                };
                if widgets::secondary(ui, text, !app.busy).clicked() {
                    if armed {
                        app.reset_armed = None;
                        app.send(Cmd::ResetAll(app.reset_lighting));
                    } else {
                        app.reset_armed = Some(Instant::now());
                    }
                }
                if armed {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                    if ui.button("Отмена").clicked() {
                        app.reset_armed = None;
                    }
                }
            },
        );
        ui.checkbox(&mut app.reset_lighting, "И подсветку тоже");
    });
}

pub fn combo(
    ui: &mut egui::Ui,
    id: &str,
    current: u8,
    items: &[(u8, &str)],
    mut on_pick: impl FnMut(u8),
) {
    let text = items
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, t)| t.to_string())
        .unwrap_or(format!("({current})"));
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(200.0)
        .show_ui(ui, |ui| {
            for (v, t) in items {
                if ui.selectable_label(*v == current, *t).clicked() && *v != current {
                    on_pick(*v);
                }
            }
        });
}
