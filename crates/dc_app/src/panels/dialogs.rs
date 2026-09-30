// =============================================================================
// dc_app/panels/dialogs - Modal Dialogs
// =============================================================================
// Popup dialogs for license entry, about, settings, etc.
// =============================================================================

use crate::state::AppState;
use egui::{Context, RichText};

/// Show all active dialogs.
pub fn show_dialogs(ctx: &Context, state: &mut AppState) {
    show_about_dialog(ctx, state);
    show_license_dialog(ctx, state);
    show_settings_dialog(ctx, state);
    show_flatten_dialog(ctx, state);
}

/// Show the Flatten confirmation dialog.
fn show_flatten_dialog(ctx: &Context, state: &mut AppState) {
    if state.ui.show_flatten_dialog {
        egui::Window::new(crate::i18n::tr("dlg.flatten_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.heading(crate::i18n::tr("dlg.flatten_heading"));
                ui.add_space(10.0);
                ui.label(crate::i18n::tr("dlg.flatten_desc1"));
                ui.label(crate::i18n::tr("dlg.flatten_desc2"));
                ui.label(crate::i18n::tr("dlg.flatten_desc3"));
                ui.add_space(15.0);

                ui.horizontal(|ui| {
                    if ui
                        .button(crate::i18n::tr("dlg.keep_merge"))
                        .on_hover_text(crate::i18n::tr("dlg.keep_merge_tip"))
                        .clicked()
                    {
                        state.session.flatten_layers(true);
                        state.ui.show_flatten_dialog = false;
                    }

                    if ui
                        .button(crate::i18n::tr("dlg.discard"))
                        .on_hover_text(crate::i18n::tr("dlg.discard_tip"))
                        .clicked()
                    {
                        state.session.flatten_layers(false);
                        state.ui.show_flatten_dialog = false;
                    }

                    if ui.button(crate::i18n::tr("dlg.cancel")).clicked() {
                        state.ui.show_flatten_dialog = false;
                    }
                });
            });
    }
}

/// Show the About dialog.
fn show_about_dialog(ctx: &Context, state: &mut AppState) {
    egui::Window::new(crate::i18n::tr("dlg.about_title"))
        .open(&mut state.ui.show_about_dialog)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                ui.heading(crate::i18n::tr("dlg.about_heading"));

                ui.add_space(5.0);
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));

                ui.add_space(15.0);
                ui.label(crate::i18n::tr("dlg.about_desc1"));
                ui.label(crate::i18n::tr("dlg.about_desc2"));

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                ui.label(RichText::new(crate::i18n::tr("dlg.tech_stack")).strong());
                ui.label("• Rust 2021 Edition");
                ui.label("• egui/eframe for cross-platform GUI");
                ui.label("• OpenCV for feature detection");
                ui.label("• PDFium for PDF rendering");
                ui.label("• Ed25519 for license validation");

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                ui.label(crate::i18n::tr("dlg.copyright"));
                ui.label(crate::i18n::tr("dlg.rights"));

                ui.add_space(10.0);
            });
        });
}

/// Show the License dialog.
fn show_license_dialog(ctx: &Context, state: &mut AppState) {
    let mut show_dialog = state.ui.show_license_dialog;

    egui::Window::new(crate::i18n::tr("dlg.license_title"))
        .open(&mut show_dialog)
        .collapsible(false)
        .resizable(true)
        .min_width(500.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.add_space(10.0);

            // Current license status
            ui.group(|ui| {
                ui.heading(crate::i18n::tr("dlg.license_status"));
                ui.add_space(5.0);

                if let Some(status) = &state.license.status {
                    if status.is_valid {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(crate::i18n::tr("dlg.valid_license"))
                                    .color(egui::Color32::GREEN)
                                    .strong(),
                            );
                        });
                        ui.label(format!("Licensee: {}", status.licensee));

                        if let Some(days) = status.days_remaining {
                            if days > 0 {
                                ui.label(format!("Expires in: {} days", days));
                            } else {
                                ui.label(
                                    RichText::new(format!("Expired {} days ago", -days))
                                        .color(egui::Color32::RED),
                                );
                            }
                        } else {
                            ui.label(crate::i18n::tr("dlg.perpetual"));
                        }

                        ui.add_space(5.0);
                        ui.label(crate::i18n::tr("dlg.licensed_features"));
                        for feature in &status.features {
                            ui.label(format!("  • {}", feature.display_name()));
                        }
                    } else {
                        ui.label(
                            RichText::new(crate::i18n::tr("dlg.invalid_license"))
                                .color(egui::Color32::RED)
                                .strong(),
                        );
                        for warning in &status.warnings {
                            ui.label(format!("  ! {}", warning));
                        }
                    }
                } else {
                    ui.label(
                        RichText::new(crate::i18n::tr("dlg.no_license"))
                            .color(egui::Color32::YELLOW),
                    );
                    if let Some(error) = &state.license.error {
                        ui.label(
                            RichText::new(format!("Error: {}", error)).color(egui::Color32::RED),
                        );
                    }
                }
            });

            ui.add_space(15.0);

            // Hardware ID display
            ui.group(|ui| {
                ui.heading(crate::i18n::tr("dlg.hwid_title"));
                ui.add_space(5.0);

                ui.label(crate::i18n::tr("dlg.hwid_desc"));
                ui.add_space(5.0);

                if let Some(hwid) = &state.license.local_hwid {
                    let hwid_display = &hwid[..32.min(hwid.len())];
                    let hwid_full = hwid.clone();
                    ui.horizontal(|ui| {
                        ui.monospace(hwid_display);
                        ui.label("...");
                        if ui.small_button(crate::i18n::tr("dlg.hwid_copy")).clicked() {
                            ui.output_mut(|o| o.copied_text = hwid_full);
                            state.ui.set_status(crate::i18n::tr("status.hwid_copied"));
                        }
                    });
                } else {
                    ui.label(
                        RichText::new(crate::i18n::tr("dlg.hwid_fail")).color(egui::Color32::RED),
                    );
                }
            });

            ui.add_space(15.0);

            // License key entry
            ui.group(|ui| {
                ui.heading(crate::i18n::tr("dlg.enter_key"));
                ui.add_space(5.0);

                ui.label(crate::i18n::tr("dlg.paste_key"));

                ui.add(
                    egui::TextEdit::multiline(&mut state.ui.license_input)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4)
                        .font(egui::TextStyle::Monospace),
                );

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("dlg.verify")).clicked() {
                        let key = state.ui.license_input.clone();
                        state.license.verify(&key);
                    }

                    if ui.button(crate::i18n::tr("dlg.clear")).clicked() {
                        state.ui.license_input.clear();
                    }
                });
            });

            ui.add_space(10.0);
        });

    state.ui.show_license_dialog = show_dialog;
}

/// Show the Settings dialog.
fn show_settings_dialog(ctx: &Context, state: &mut AppState) {
    egui::Window::new(crate::i18n::tr("dlg.settings_title"))
        .open(&mut state.ui.show_settings_dialog)
        .collapsible(false)
        .resizable(false)
        .min_width(400.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            super::properties_panel::section(ui, crate::i18n::tr("settings.general"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("settings.language"));
                    egui::ComboBox::from_id_salt("settings_language")
                        .selected_text(state.ui.language.label())
                        .show_ui(ui, |ui| {
                            for &language in crate::i18n::Language::all() {
                                if ui
                                    .selectable_value(
                                        &mut state.ui.language,
                                        language,
                                        language.label(),
                                    )
                                    .changed()
                                {
                                    crate::i18n::set_language(language);
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("settings.processing"));
                    let before = state.compute_mode;
                    egui::ComboBox::from_id_salt("settings_processing")
                        .selected_text(crate::i18n::tr(match before {
                            crate::state::ComputeMode::Auto => "settings.automatic",
                            crate::state::ComputeMode::Gpu => "settings.gpu",
                            crate::state::ComputeMode::Cpu => "settings.cpu",
                        }))
                        .show_ui(ui, |ui| {
                            for (mode, key) in [
                                (crate::state::ComputeMode::Auto, "settings.automatic"),
                                (crate::state::ComputeMode::Gpu, "settings.gpu"),
                                (crate::state::ComputeMode::Cpu, "settings.cpu"),
                            ] {
                                ui.add_enabled_ui(
                                    mode != crate::state::ComputeMode::Gpu
                                        || state.gpu_engine.is_some(),
                                    |ui| {
                                        ui.selectable_value(
                                            &mut state.compute_mode,
                                            mode,
                                            crate::i18n::tr(key),
                                        );
                                    },
                                );
                            }
                        });
                    if before != state.compute_mode {
                        state.ui.diff_invalidated = true;
                    }
                });
            });

            let mut workflow_changed = false;
            // Workflow States configuration
            super::properties_panel::section(ui, crate::i18n::tr("dlg.workflow_states"), |ui| {
                ui.label(crate::i18n::tr("dlg.workflow_desc"));
                ui.add_space(5.0);

                let mut to_remove = None;

                for (idx, state) in state.session.workflow_states.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}.", idx + 1));

                        let mut name = state.name.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut name).desired_width(120.0))
                            .changed()
                        {
                            state.name = name;
                        }

                        let mut color = [
                            state.color[0] as f32 / 255.0,
                            state.color[1] as f32 / 255.0,
                            state.color[2] as f32 / 255.0,
                        ];
                        if ui.color_edit_button_rgb(&mut color).changed() {
                            state.color = [
                                (color[0] * 255.0).round() as u8,
                                (color[1] * 255.0).round() as u8,
                                (color[2] * 255.0).round() as u8,
                            ];
                        }

                        if ui.small_button("×").clicked() {
                            to_remove = Some(idx);
                        }
                    });
                }

                if let Some(idx) = to_remove {
                    let removed = state.session.workflow_states.remove(idx);
                    // Clean up usages of this ID
                    state
                        .session
                        .annotation_statuses
                        .retain(|_, v| v != &removed.id);
                    state.session.is_dirty = true;
                }

                ui.add_space(5.0);
                if ui.button(crate::i18n::tr("dlg.add_state")).clicked() {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    state
                        .session
                        .workflow_states
                        .push(crate::state::WorkflowState {
                            id: new_id,
                            name: crate::i18n::tr("dlg.new_state").to_string(),
                            color: [128, 128, 128],
                        });
                    state.session.is_dirty = true;
                }
            });
            state.session.is_dirty |= workflow_changed;
        });
}
