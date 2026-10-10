//! Copies of a profile: into another slot, to a file on this computer and
//! back from one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use apex5::library;
use apex5::worker::Cmd;
use eframe::egui::{self, RichText, vec2};

use super::theme::{self, DIM, FAINT, TEXT, WARN};
use super::widgets::{self, card_titled, hint};
use crate::App;

/// How long a first press waits for the second one.
const CONFIRM: Duration = Duration::from_secs(5);

/// What a first press asked for.
#[derive(Clone, PartialEq)]
pub enum Pending {
    CopyTo(u8),
    Load(PathBuf),
}

/// True on the confirming press; the first press only arms `what`.
fn confirmed(app: &mut App, what: Pending) -> bool {
    let armed = app
        .copy_armed
        .as_ref()
        .is_some_and(|(w, t)| *w == what && t.elapsed() < CONFIRM);
    app.copy_armed = if armed {
        None
    } else {
        Some((what, Instant::now()))
    };
    armed
}

fn is_armed(app: &App, what: &Pending) -> bool {
    app.copy_armed
        .as_ref()
        .is_some_and(|(w, t)| w == what && t.elapsed() < CONFIRM)
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.copy_armed.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    let led_pending = app.led.as_ref().is_some_and(|l| l.dirty_since.is_some());
    let unsaved = app.profile_dirty() || app.flash_dirty || app.led_flash_dirty || led_pending;
    let idle = !app.busy;
    let here = app.active;

    card_titled(
        ui,
        "Копировать в другой профиль",
        |ui| {
            hint(
                ui,
                &format!(
                    "Профиль {} вместе с подсветкой, как они сохранены в геймпаде, заменит выбранный \
                 профиль и сохранится в нём.",
                    here + 1
                ),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for slot in (0..apex5::profile::SLOTS).filter(|s| *s != here) {
                    let what = Pending::CopyTo(slot);
                    let text = if is_armed(app, &what) {
                        format!("Заменить профиль {}?", slot + 1)
                    } else {
                        format!("В профиль {}", slot + 1)
                    };
                    if widgets::secondary(ui, &text, idle && !unsaved).clicked()
                        && confirmed(app, what)
                        && let Some(p) = app.pad_profile.clone()
                    {
                        let led = app.led.as_ref().map(|l| l.cfg.clone());
                        app.send(Cmd::Install {
                            profile: p,
                            led,
                            slot,
                        });
                    }
                }
            });
            if unsaved {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Сначала сохраните изменения этого профиля и подсветки.")
                        .size(12.5)
                        .color(WARN),
                );
            }
        },
    );

    ui.add_space(12.0);
    card_titled(ui, "Сохранить на компьютер", |ui| {
        hint(
            ui,
            "Профиль и подсветка в том виде, как они сейчас на экране, сохраняются в файл.",
        );
        ui.add_space(6.0);
        if app.save_name.is_empty() {
            app.save_name = app.title.clone();
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.save_name)
                    .desired_width(240.0)
                    .hint_text("название файла")
                    .margin(vec2(10.0, 7.0)),
            );
            if widgets::secondary(ui, "Сохранить", true).clicked() {
                save(app);
            }
        });
    });

    ui.add_space(12.0);
    card_titled(
        ui,
        "Сохранённые на компьютере",
        |ui| {
            let folder = library::dir();
            ui.horizontal(|ui| {
                if let Some(d) = &folder {
                    hint(ui, &d.display().to_string());
                }
                if widgets::secondary(ui, "Открыть папку", folder.is_some()).clicked()
                    && let Some(d) = &folder
                {
                    let _ = std::fs::create_dir_all(d);
                    let _ = std::process::Command::new("xdg-open").arg(d).spawn();
                }
                if widgets::secondary(ui, "Обновить", true).clicked() {
                    app.library = None;
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut app.load_lighting, "Загружать вместе с подсветкой");
            ui.add_space(6.0);
            let entries = app.library.get_or_insert_with(library::list);
            if entries.is_empty() {
                ui.label(RichText::new("Пока ничего не сохранено").color(FAINT));
                return;
            }
            let mut load = None;
            let rows: Vec<_> = entries
                .iter()
                .map(|e| {
                    (
                        e.path.clone(),
                        e.name.clone(),
                        e.title.clone(),
                        e.has_lighting,
                    )
                })
                .collect();
            for (i, (path, name, title, lit)) in rows.into_iter().enumerate() {
                if i > 0 {
                    widgets::separator(ui);
                }
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(&name).font(theme::medium(14.0)).color(TEXT));
                        let sub = if lit {
                            format!("«{title}», с подсветкой")
                        } else {
                            format!("«{title}»")
                        };
                        ui.label(RichText::new(sub).size(12.0).color(DIM));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let what = Pending::Load(path.clone());
                        let text = if is_armed(app, &what) {
                            format!("Заменить профиль {}?", here + 1)
                        } else {
                            format!("Загрузить в профиль {}", here + 1)
                        };
                        if widgets::secondary(ui, &text, idle).clicked() && confirmed(app, what) {
                            load = Some(path.clone());
                        }
                    });
                });
            }
            if let Some(path) = load {
                match library::load(&path) {
                    Ok((profile, led)) => app.send(Cmd::Install {
                        profile,
                        led: led.filter(|_| app.load_lighting),
                        slot: here,
                    }),
                    Err(e) => app.toast = Some((format!("{e:#}"), true, Instant::now())),
                }
            }
        },
    );
}

fn save(app: &mut App) {
    let Some(p) = app.edited() else { return };
    let led = app.led.as_ref().map(|l| l.cfg.clone());
    let kind = app.info.as_ref().map_or(0, |i| i.device_type);
    let msg = match library::save(&app.save_name, &p, led.as_ref(), kind) {
        Ok(path) => (format!("Сохранено: {}", path.display()), false),
        Err(e) => (format!("{e:#}"), true),
    };
    app.toast = Some((msg.0, msg.1, Instant::now()));
    app.library = None;
}
