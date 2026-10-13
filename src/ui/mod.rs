//! Window layout, overview and device settings.

use std::time::{Duration, Instant};

use apex5::device::{self, NICKNAME_MAX};
use apex5::lang::{self, key, t};
use apex5::tr;
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

use theme::accent;
use widgets::{Icon, card, card_titled, page_title, row, toggle_row};

/// The menu: page, icon, name.
const NAV: [(Tab, Icon, &str); 7] = [
    (Tab::Overview, Icon::Home, key("Controller")),
    (Tab::Lighting, Icon::Light, key("Lighting")),
    (Tab::Screen, Icon::Screen, key("Screen")),
    (Tab::Profile, Icon::Profiles, key("Profiles")),
    (Tab::DualSense, Icon::Waves, key("DualSense mode")),
    (Tab::Input, Icon::Input, key("Input test")),
    (Tab::Settings, Icon::Settings, key("Settings")),
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
            .frame(egui::Frame::new().fill(theme::bg()))
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
                .fill(theme::bg())
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
                .fill(theme::bg())
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
                if window_button(ui, Icon::Close, t("Close"), true).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let (icon, tip) = if max {
                    (Icon::Restore, t("Restore"))
                } else {
                    (Icon::Maximize, t("Maximize"))
                };
                if window_button(ui, icon, tip, false).clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
                if window_button(ui, Icon::Minimize, t("Minimize"), false).clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                ui.add_space(14.0);
                if refresh {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if widgets::icon_button(
                        ui,
                        Icon::Refresh,
                        t("Read everything from the controller again"),
                    )
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
        (true, false) => (theme::hover(), theme::text()),
        _ => (Color32::TRANSPARENT, theme::dim()),
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
        Stroke::new(1.0, theme::border()),
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
                .fill(theme::sidebar())
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
                        .color(theme::silver()),
                );
                ui.label(RichText::new("5").font(theme::bold(24.0)).color(accent()));
            });
            ui.label(
                RichText::new("C O N T R O L")
                    .font(theme::medium(10.5))
                    .color(theme::faint()),
            );
            ui.add_space(16.0);

            device_badge(app, ui);
            ui.add_space(14.0);

            for (tab, icon, name) in NAV {
                if NAV_BREAKS.contains(&tab) {
                    ui.add_space(3.0);
                    widgets::separator(ui);
                    ui.add_space(5.0);
                }
                nav_item(app, ui, tab, icon, t(name));
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    widgets::icon(ui.painter(), rect, Icon::Shield, theme::faint());
                    ui.label(
                        RichText::new(t("Protected mode"))
                            .size(12.0)
                            .color(theme::faint()),
                    );
                })
                .response
                .on_hover_text(t(
                    "Dangerous commands are blocked: the app never sends firmware, factory \
                     reset, mode change or device type commands. Every packet is checked \
                     against an allowlist.",
                ));
            });
        });
}

/// The pad's state and its four profiles, on every page.
fn device_badge(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::card())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            let Some(i) = app.info.clone() else {
                ui.horizontal(|ui| {
                    status_dot(ui, theme::bad());
                    ui.label(
                        RichText::new(t("Not connected"))
                            .font(theme::medium(14.0))
                            .color(theme::text()),
                    );
                });
                ui.label(
                    RichText::new(t("waiting for the controller…"))
                        .size(12.5)
                        .color(theme::dim()),
                );
                return;
            };
            ui.horizontal(|ui| {
                status_dot(ui, theme::ok());
                ui.label(
                    RichText::new(format!("Flydigi {}", i.model()))
                        .font(theme::medium(14.0))
                        .color(theme::text()),
                );
            });
            ui.horizontal(|ui| {
                widgets::battery(ui, i.battery_level, i.charging);
                let b = format!("{}%", i.battery_level as u32 * 20);
                ui.label(RichText::new(b).size(12.5).color(theme::dim()));
                ui.label(RichText::new("·").color(theme::faint()));
                ui.label(
                    RichText::new(if i.wired { t("cable") } else { t("dongle") })
                        .size(12.5)
                        .color(theme::dim()),
                );
            });
            ui.add_space(6.0);
            profile_switch(app, ui);
            if let Some(p) = &app.pad_profile {
                let shown = profile::shown_title(&p.title());
                ui.add(
                    egui::Label::new(
                        RichText::new(if shown.is_empty() {
                            t("untitled").into()
                        } else {
                            shown
                        })
                        .size(12.5)
                        .color(theme::dim()),
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
            let on = ui.ctx().animate_bool_responsive(r.id, active);
            let bg = if r.hovered() && !active {
                theme::hover()
            } else {
                theme::card_hi()
            };
            ui.painter().rect(
                rect,
                CornerRadius::same(8),
                widgets::lerp_colour(bg, accent(), on),
                Stroke::new(1.0, widgets::lerp_colour(theme::border(), accent(), on)),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("P{}", slot + 1),
                theme::medium(13.5),
                if active {
                    theme::on_accent()
                } else {
                    theme::dim()
                },
            );
            let r = r.on_hover_text(if active {
                t("Active profile")
            } else {
                t("Switch the controller to this profile")
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
    let on = ui.ctx().animate_bool_responsive(r.id, sel);
    let p = ui.painter();
    if sel || r.hovered() {
        let bg = if sel { theme::card_hi() } else { theme::card() };
        p.rect_filled(rect, CornerRadius::same(10), bg);
    }
    if on > 0.0 {
        let bar = egui::Rect::from_min_size(
            pos2(rect.left(), rect.center().y - 10.0 * on),
            vec2(3.0, 20.0 * on),
        );
        p.rect_filled(bar, CornerRadius::same(2), accent());
    }
    let colour = if sel {
        accent()
    } else if r.hovered() {
        theme::text()
    } else {
        theme::dim()
    };
    let ir =
        egui::Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(18.0, 18.0));
    widgets::icon(p, ir, icon, colour);
    p.text(
        pos2(rect.left() + 48.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::medium(14.5),
        if sel { theme::text() } else { colour },
    );
    if tab == Tab::Profile && (app.profile_dirty() || app.flash_dirty) {
        p.circle_filled(
            pos2(rect.right() - 16.0, rect.center().y),
            4.0,
            theme::warn(),
        );
        r.clone()
            .on_hover_text(t("The profile has unsaved changes"));
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
    let colour = if *err { theme::bad() } else { theme::ok() };
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
                .fill(theme::card_hi())
                .stroke(Stroke::new(1.0, colour.gamma_multiply(0.6)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(16, 12))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: theme::palette().shadow,
                })
                .show(ui, |ui| {
                    ui.set_max_width(380.0);
                    ui.horizontal(|ui| {
                        status_dot(ui, colour);
                        ui.label(RichText::new(msg).color(theme::text()));
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
            RichText::new(t("Controller not found"))
                .font(theme::bold(24.0))
                .color(theme::text()),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(t(
                "Connect the Apex 5 by cable or through the dongle — the app will find it.",
            ))
            .color(theme::dim()),
        );
        ui.label(RichText::new(&app.status).size(12.5).color(theme::faint()));
        ui.add_space(4.0);
        ui.label(
            RichText::new(t(
                "If the controller is connected but there is no access, install the udev rule.",
            ))
            .size(12.5)
            .color(theme::faint()),
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
        ui.add_space(20.0);
        language_menu(ui);
    });
}

// ------------------------------------------------------------------ overview

fn overview(app: &mut App, ui: &mut egui::Ui) {
    let Some(info) = app.info.clone() else { return };
    header(
        app,
        ui,
        &format!("Flydigi {}", info.model()),
        t("Controller state and quick actions"),
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
    card_titled(ui, t("About the device"), |ui| {
        egui::Grid::new("about")
            .num_columns(2)
            .spacing([28.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new(t("Model")).color(theme::dim()));
                ui.label(tr!(
                    "Flydigi {model} (type {kind})",
                    model = info.model(),
                    kind = info.device_type
                ));
                ui.end_row();
                ui.label(RichText::new("UID").color(theme::dim()));
                ui.monospace(&info.uid);
                ui.end_row();
                for (name, v) in &info.firmware {
                    ui.label(RichText::new(tr!("Firmware: {name}", name)).color(theme::dim()));
                    ui.monospace(v);
                    ui.end_row();
                }
                ui.label(RichText::new(t("Name")).color(theme::dim()))
                    .on_hover_text(t(
                        "A service name the controller keeps in itself. Flydigi Space Station \
                         and other programs show it to tell several controllers apart. It is not \
                         shown on the controller's screen or in games.",
                    ));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut app.nickname)
                            .desired_width(200.0)
                            .hint_text(t("no name"))
                            .margin(vec2(10.0, 5.0)),
                    );
                    let n = app.nickname.len();
                    ui.label(
                        RichText::new(format!("{n}/{NICKNAME_MAX}"))
                            .size(12.0)
                            .color(theme::faint()),
                    );
                    let ok = n > 0 && n <= NICKNAME_MAX && app.nickname != info.nickname;
                    if ok && widgets::secondary(ui, t("Save"), true).clicked() {
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
            t("Apply or revert the profile changes first").into(),
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
            tr!("Profile P{slot}", slot = app.active + 1)
        } else {
            tr!("Profile P{slot} · {title}", slot = app.active + 1, title)
        };
        card_titled(ui, &caption, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (tab, name, value, notable) in profile::summary(&p) {
                let colour = if notable { accent() } else { theme::text() };
                if widgets::link_row(ui, name, &value, colour).clicked() {
                    app.tab = Tab::Profile;
                    app.profile_tab = tab;
                }
            }
        });
        ui.add_space(12.0);
    }
    card_titled(ui, t("Lighting"), |ui| {
        if let Some(led) = app.led.as_mut() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(lighting::effect_name(led.effect))
                        .font(theme::medium(14.5))
                        .color(theme::text()),
                );
                for c in led.colours.iter().take(5) {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    ui.painter().circle(
                        rect.center(),
                        7.0,
                        Color32::from_rgb(c[0], c[1], c[2]),
                        Stroke::new(1.0, theme::border()),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Brightness")).color(theme::dim()));
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
            if ui.link(t("Set up lighting →")).clicked() {
                app.tab = Tab::Lighting;
            }
            if app.led_dirty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::secondary(ui, t("Save"), !app.busy)
                        .on_hover_text(t(
                            "Unsaved lighting is lost when the controller goes to sleep",
                        ))
                        .clicked()
                    {
                        app.save_led();
                    }
                    ui.label(
                        RichText::new(t("not saved"))
                            .size(12.5)
                            .color(theme::warn()),
                    );
                });
            }
        });
    });
}

// ------------------------------------------------------------------ settings

/// Switches of the pad: feature number, name, hint.
type Feature = (u8, &'static str, &'static str);

fn button_features() -> [Feature; 2] {
    [
        (
            1,
            t("Quick profile switch"),
            t("Fn + A/B/X/Y switches the profile right on the controller"),
        ),
        (
            4,
            t("Turbo button"),
            t(
                "Turbo + a button turns turbo on from the controller. Turbo from the profile works without it",
            ),
        ),
    ]
}

fn stick_features() -> [Feature; 3] {
    [
        (
            5,
            t("Jitter filter"),
            t("Smooths out jitter; auto calibration does not work without it"),
        ),
        (
            6,
            t("Auto calibration"),
            t("Adjusts the sticks' centre during play"),
        ),
        (
            7,
            t("Rebound filter"),
            t("Damps the overshoot when a stick is released sharply"),
        ),
    ]
}

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
        t("Settings"),
        t("Applied at once and kept in the controller itself"),
    );
    pad_settings(app, ui);
    ui.add_space(12.0);
    app_settings(ui);
}

/// What this app keeps on the computer.
fn app_settings(ui: &mut egui::Ui) {
    card_titled(ui, t("App"), |ui| {
        // findable in English too, should a language be picked by mistake
        let title = if lang::current() == lang::ENGLISH {
            "Language".to_string()
        } else {
            format!("{} · Language", t("Language"))
        };
        row(ui, &title, "", language_menu);
        widgets::separator(ui);
        row(ui, t("Theme"), "", |ui| {
            let mut c = theme::choice();
            let items = [
                (theme::Choice::System, t("As in the system#theme")),
                (theme::Choice::Dark, t("Dark#theme")),
                (theme::Choice::Light, t("Light#theme")),
            ];
            if widgets::segmented(ui, &mut c, &items) {
                theme::pick(ui.ctx(), c);
            }
        });
    });
}

/// The languages by their own names.
fn language_menu(ui: &mut egui::Ui) {
    let now = lang::current();
    let languages = lang::languages();
    let name = languages
        .iter()
        .find(|(c, _)| *c == now)
        .map_or(now, |(_, n)| n);
    egui::ComboBox::from_id_salt("lang")
        .selected_text(name)
        .width(200.0)
        .show_ui(ui, |ui| {
            for (code, name) in languages {
                if ui.selectable_label(code == now, name).clicked() && code != now {
                    lang::pick(code);
                }
            }
        });
}

/// The pad's own settings and the reset.
fn pad_settings(app: &mut App, ui: &mut egui::Ui) {
    let Some(s) = app.settings.clone() else {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    };

    card_titled(ui, t("Buttons"), |ui| {
        features(app, ui, &s, &button_features())
    });

    ui.add_space(12.0);
    card_titled(ui, t("Sticks"), |ui| {
        features(app, ui, &s, &stick_features());
        widgets::separator(ui);
        row(
            ui,
            t("Precision"),
            t("Resolution of the regular (Xbox) input. Flydigi offers 8–12 bits"),
            |ui| {
                combo(ui, "prec", s.precision, &device::precision(), |v| {
                    app.send(Cmd::SetNumber(21, v))
                });
            },
        );
        widgets::separator(ui);
        row(
            ui,
            t("Centre sensitivity"),
            t("How early a stick responds to a tilt"),
            |ui| {
                combo(ui, "sens", s.sensitivity, &device::sensitivity(), |v| {
                    app.send(Cmd::SetNumber(22, v))
                });
            },
        );
    });

    ui.add_space(12.0);
    card_titled(ui, t("Power"), |ui| {
        row(
            ui,
            t("Auto sleep"),
            t("After how many idle minutes the controller goes to sleep"),
            |ui| {
                // a value set elsewhere stays pickable alongside the presets
                let custom = tr!("{minutes} min", minutes = s.sleep_minutes);
                let mut items = device::sleep();
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
        card_titled(ui, t("Third-party programs"), |ui| {
            let mut on = f.third_party;
            let r = ui.scope(|ui| {
                toggle_row(
                    ui,
                    t("Allow layout takeover"),
                    t("Steam and other SDL programs can take over the controller"),
                    &mut on,
                )
            });
            r.response.on_hover_text(t(
                "While a program holds the controller, the profile does not apply and the \
                 regular input (xpad) is gone: games see the controller only through that program.",
            ));
            if r.inner {
                app.send(Cmd::SetThirdParty(on));
            }
            if !f.control_by.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(tr!("The controller is taken over by {program}. The profile does not apply, regular input is off.", program = f.control_by))
                    .color(theme::warn()),
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
    card_titled(ui, t("Reset"), |ui| {
        let armed = app.reset_armed.is_some_and(|t| t.elapsed() < WAIT);
        row(
            ui,
            t("All profiles to factory"),
            t("Buttons, macros, sticks, triggers, vibration, gyro and names of all four profiles"),
            |ui| {
                let text = if armed {
                    t("Really reset?")
                } else {
                    t("Reset…")
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
                    if ui.button(t("Cancel")).clicked() {
                        app.reset_armed = None;
                    }
                }
            },
        );
        ui.checkbox(&mut app.reset_lighting, t("Lighting too"));
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
