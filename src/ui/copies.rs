//! Copies of a profile: into another slot, to a file on this computer and
//! back from one, from the menu next to the profile tabs.

use std::path::PathBuf;
use std::time::Instant;

use apex5::library;
use apex5::worker::Cmd;
use eframe::egui::{self, RichText, vec2};

use super::theme::{self, DIM, FAINT, TEXT};
use super::widgets::{self, hint};
use crate::App;

/// A dialog the profile menu opened.
#[derive(Clone, PartialEq)]
pub enum Dialog {
    /// Copy the active profile into another slot.
    CopyTo(u8),
    /// Save the edited profile to a file.
    Save,
    /// Pick a saved profile, then confirm loading it.
    Library(Option<PathBuf>),
}

/// Unsaved work that a copy would leave behind.
fn unsaved(app: &App) -> bool {
    let led_pending = app.led.as_ref().is_some_and(|l| l.dirty_since.is_some());
    app.profile_dirty() || app.flash_dirty || app.led_flash_dirty || led_pending
}

/// The "⋯" menu next to the profile tabs: copies, files, factory values.
pub fn menu(app: &mut App, ui: &mut egui::Ui) {
    let r = widgets::icon_button(ui, widgets::Icon::More, "Копии, файлы, заводские значения");
    let here = app.active;
    let idle = !app.busy;
    let unsaved = unsaved(app);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(240.0);
        for slot in (0..apex5::profile::SLOTS).filter(|s| *s != here) {
            let b = ui
                .add_enabled(
                    idle && !unsaved,
                    egui::Button::new(format!("Копировать в профиль {}", slot + 1)),
                )
                .on_disabled_hover_text("Сначала сохраните изменения профиля и подсветки");
            if b.clicked() {
                app.copy_dialog = Some(Dialog::CopyTo(slot));
            }
        }
        ui.separator();
        if ui.button("Сохранить в файл…").clicked() {
            if app.save_name.is_empty() {
                app.save_name = super::profile::shown_title(&app.title);
            }
            app.copy_dialog = Some(Dialog::Save);
        }
        if ui
            .add_enabled(idle, egui::Button::new("Загрузить из файла…"))
            .clicked()
        {
            app.library = None;
            app.copy_dialog = Some(Dialog::Library(None));
        }
        ui.separator();
        if ui
            .button("Заводские значения")
            .on_hover_text(
                "Вернуть заводские значения этого профиля, вибрацию тоже. Нужно будет применить.",
            )
            .clicked()
            && let Some(e) = app.edit.as_mut()
        {
            e.factory_reset();
            app.macros = e.macros();
            app.title = e.title();
        }
    });
}

fn modal(ui: &egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) -> bool {
    egui::Modal::new(egui::Id::new("copies_modal"))
        .frame(
            egui::Frame::new()
                .fill(theme::CARD)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(egui::CornerRadius::same(14))
                .inner_margin(egui::Margin::same(20)),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(title).font(theme::bold(18.0)).color(TEXT));
            ui.add_space(6.0);
            add(ui);
        })
        .should_close()
}

/// The open dialog, over everything else.
pub fn dialogs(app: &mut App, ui: &mut egui::Ui) {
    let Some(d) = app.copy_dialog.clone() else {
        return;
    };
    let here = app.active;
    let mut close = false;
    let dismissed = match d {
        Dialog::CopyTo(slot) => modal(
            ui,
            &format!("Заменить профиль {}?", slot + 1),
            |ui| {
                hint(
                    ui,
                    &format!(
                        "Профиль {} вместе с подсветкой, как они сохранены в геймпаде, \
                     заменит профиль {} и сохранится в нём.",
                        here + 1,
                        slot + 1
                    ),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if widgets::primary(ui, "Заменить", !app.busy).clicked()
                        && let Some(p) = app.pad_profile.clone()
                    {
                        let led = app.led.as_ref().map(|l| l.cfg.clone());
                        app.send(Cmd::Install {
                            profile: p,
                            led,
                            slot,
                        });
                        close = true;
                    }
                    if widgets::secondary(ui, "Отмена", true).clicked() {
                        close = true;
                    }
                });
            },
        ),
        Dialog::Save => modal(ui, "Сохранить профиль в файл", |ui| {
            hint(
                ui,
                "Профиль и подсветка в том виде, как они сейчас на экране.",
            );
            ui.add_space(8.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut app.save_name)
                    .desired_width(f32::INFINITY)
                    .hint_text("название файла")
                    .margin(vec2(10.0, 7.0)),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(d) = library::dir() {
                ui.label(
                    RichText::new(format!("в папку {}", d.display()))
                        .size(12.0)
                        .color(FAINT),
                );
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let ok = !app.save_name.trim().is_empty();
                if (widgets::primary(ui, "Сохранить", ok).clicked() || (enter && ok)) && save(app)
                {
                    close = true;
                }
                if widgets::secondary(ui, "Отмена", true).clicked() {
                    close = true;
                }
            });
        }),
        Dialog::Library(None) => modal(
            ui,
            "Загрузить профиль из файла",
            |ui| {
                close = library_list(app, ui);
            },
        ),
        Dialog::Library(Some(path)) => {
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            modal(
                ui,
                &format!("Заменить профиль {}?", here + 1),
                |ui| {
                    hint(
                        ui,
                        &format!(
                            "Профиль {} заменит «{name}»{} и сохранится в геймпаде.",
                            here + 1,
                            if app.load_lighting {
                                " вместе с подсветкой"
                            } else {
                                ""
                            }
                        ),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Заменить", !app.busy).clicked() {
                            match library::load(&path) {
                                Ok((profile, led)) => app.send(Cmd::Install {
                                    profile,
                                    led: led.filter(|_| app.load_lighting),
                                    slot: here,
                                }),
                                Err(e) => {
                                    app.toast = Some((format!("{e:#}"), true, Instant::now()))
                                }
                            }
                            close = true;
                        }
                        if widgets::secondary(ui, "Назад", true).clicked() {
                            app.copy_dialog = Some(Dialog::Library(None));
                        }
                    });
                },
            )
        }
    };
    if close || dismissed {
        app.copy_dialog = None;
    }
}

/// Saved profiles to pick from. Returns true to close the dialog.
fn library_list(app: &mut App, ui: &mut egui::Ui) -> bool {
    let mut close = false;
    ui.checkbox(&mut app.load_lighting, "Вместе с подсветкой");
    ui.add_space(6.0);
    let entries = app.library.get_or_insert_with(library::list);
    if entries.is_empty() {
        ui.label(RichText::new("Пока ничего не сохранено").color(FAINT));
    }
    let mut pick = None;
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, e) in entries.iter().enumerate() {
                if i > 0 {
                    widgets::separator(ui);
                }
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(&e.name).font(theme::medium(14.0)).color(TEXT));
                        let sub = if e.has_lighting {
                            format!("«{}», с подсветкой", super::profile::shown_title(&e.title))
                        } else {
                            format!("«{}»", super::profile::shown_title(&e.title))
                        };
                        ui.label(RichText::new(sub).size(12.0).color(DIM));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::secondary(ui, "Загрузить", true).clicked() {
                            pick = Some(e.path.clone());
                        }
                    });
                });
            }
        });
    if let Some(p) = pick {
        app.copy_dialog = Some(Dialog::Library(Some(p)));
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        let folder = library::dir();
        if widgets::secondary(ui, "Открыть папку", folder.is_some()).clicked()
            && let Some(d) = &folder
        {
            let _ = std::fs::create_dir_all(d);
            let _ = std::process::Command::new("xdg-open").arg(d).spawn();
        }
        if widgets::secondary(ui, "Обновить", true).clicked() {
            app.library = None;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::secondary(ui, "Закрыть", true).clicked() {
                close = true;
            }
        });
    });
    close
}

/// Returns true when the file was written.
fn save(app: &mut App) -> bool {
    let Some(p) = app.edited() else { return false };
    let led = app.led.as_ref().map(|l| l.cfg.clone());
    let kind = app.info.as_ref().map_or(0, |i| i.device_type);
    let msg = match library::save(&app.save_name, &p, led.as_ref(), kind) {
        Ok(path) => (format!("Сохранено: {}", path.display()), false),
        Err(e) => (format!("{e:#}"), true),
    };
    let ok = !msg.1;
    app.toast = Some((msg.0, msg.1, Instant::now()));
    app.library = None;
    ok
}
