//! Copies of a profile: into another slot, to a file on this computer and
//! back from one, from the menu next to the profile tabs.

use std::path::PathBuf;
use std::time::Instant;

use apex5::lang::t;
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
    let r = widgets::icon_button(ui, widgets::Icon::More, t("Copies, files, factory values"));
    let here = app.active;
    let idle = !app.busy;
    let unsaved = unsaved(app);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(240.0);
        for slot in (0..apex5::profile::SLOTS).filter(|s| *s != here) {
            let b = ui
                .add_enabled(
                    idle && !unsaved,
                    egui::Button::new(apex5::tr!("Copy to profile {slot}", slot = slot + 1)),
                )
                .on_disabled_hover_text(t("Save the profile and lighting changes first"));
            if b.clicked() {
                app.copy_dialog = Some(Dialog::CopyTo(slot));
            }
        }
        ui.separator();
        if ui.button(t("Save to file…")).clicked() {
            if app.save_name.is_empty() {
                app.save_name = super::profile::shown_title(&app.title);
            }
            app.copy_dialog = Some(Dialog::Save);
        }
        if ui
            .add_enabled(idle, egui::Button::new(t("Load from file…")))
            .clicked()
        {
            app.library = None;
            app.copy_dialog = Some(Dialog::Library(None));
        }
        ui.separator();
        if ui
            .button(t("Factory values"))
            .on_hover_text(t(
                "Restore this profile's factory values, vibration too. It will need applying.",
            ))
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
            &apex5::tr!("Replace profile {slot}?", slot = slot + 1),
            |ui| {
                hint(
                    ui,
                    &apex5::tr!(
                        "Profile {here} with its lighting, as saved in the controller, \
                     will replace profile {slot} and be saved there.",
                        here = here + 1,
                        slot = slot + 1
                    ),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if widgets::primary(ui, t("Replace"), !app.busy).clicked()
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
                    if widgets::secondary(ui, t("Cancel"), true).clicked() {
                        close = true;
                    }
                });
            },
        ),
        Dialog::Save => modal(ui, t("Save profile to file"), |ui| {
            hint(ui, t("The profile and lighting as they are on screen now."));
            ui.add_space(8.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut app.save_name)
                    .desired_width(f32::INFINITY)
                    .hint_text(t("file name"))
                    .margin(vec2(10.0, 7.0)),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(d) = library::dir() {
                ui.label(
                    RichText::new(apex5::tr!("to the folder {dir}", dir = d.display()))
                        .size(12.0)
                        .color(FAINT),
                );
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let ok = !app.save_name.trim().is_empty();
                if (widgets::primary(ui, t("Save"), ok).clicked() || (enter && ok)) && save(app) {
                    close = true;
                }
                if widgets::secondary(ui, t("Cancel"), true).clicked() {
                    close = true;
                }
            });
        }),
        Dialog::Library(None) => modal(ui, t("Load profile from file"), |ui| {
            close = library_list(app, ui);
        }),
        Dialog::Library(Some(path)) => {
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            modal(
                ui,
                &apex5::tr!("Replace profile {here}?", here = here + 1),
                |ui| {
                    hint(ui, &{
                        let n = here + 1;
                        let with = if app.load_lighting {
                            t(" along with the lighting")
                        } else {
                            ""
                        };
                        apex5::tr!(
                            "\"{name}\"{with} will replace profile {n} and be saved in the controller.",
                            name,
                            with,
                            n
                        )
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, t("Replace"), !app.busy).clicked() {
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
                        if widgets::secondary(ui, t("Back"), true).clicked() {
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
    ui.checkbox(&mut app.load_lighting, t("Along with the lighting"));
    ui.add_space(6.0);
    let entries = app.library.get_or_insert_with(library::list);
    if entries.is_empty() {
        ui.label(RichText::new(t("Nothing saved yet")).color(FAINT));
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
                            apex5::tr!(
                                "\"{title}\", with lighting",
                                title = super::profile::shown_title(&e.title)
                            )
                        } else {
                            apex5::tr!("\"{title}\"", title = super::profile::shown_title(&e.title))
                        };
                        ui.label(RichText::new(sub).size(12.0).color(DIM));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::secondary(ui, t("Load"), true).clicked() {
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
        if widgets::secondary(ui, t("Open folder"), folder.is_some()).clicked()
            && let Some(d) = &folder
        {
            let _ = std::fs::create_dir_all(d);
            let _ = std::process::Command::new("xdg-open").arg(d).spawn();
        }
        if widgets::secondary(ui, t("Refresh"), true).clicked() {
            app.library = None;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::secondary(ui, t("Close"), true).clicked() {
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
        Ok(path) => (apex5::tr!("Saved: {path}", path = path.display()), false),
        Err(e) => (format!("{e:#}"), true),
    };
    let ok = !msg.1;
    app.toast = Some((msg.0, msg.1, Instant::now()));
    app.library = None;
    ok
}
