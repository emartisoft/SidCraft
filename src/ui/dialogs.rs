// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

use super::*;
impl SidcraftApp {
    pub(super) fn new_playlist_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let mut open = self.show_new_playlist;
        let mut submit = false;

        egui::Window::new("New playlist")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(20)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(
                    RichText::new("Create a playlist")
                        .font(theme::semibold(16.0))
                        .color(t.text),
                );
                ui.label(
                    RichText::new("The new list is saved in your playlist library and becomes active.")
                        .font(theme::regular(11.5))
                        .color(t.text_muted),
                );
                ui.add_space(10.0);

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.new_playlist_name)
                        .hint_text("Playlist name")
                        .desired_width(f32::INFINITY)
                        .font(theme::regular(13.0)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }

                if let Some(err) = &self.new_playlist_error {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(err)
                            .font(theme::regular(11.5))
                            .color(t.accent_text),
                    );
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let can_create = !self.new_playlist_name.trim().is_empty();
                    if primary_button(ui, &t, "Create").clicked() && can_create {
                        submit = true;
                    }
                    if secondary_button(ui, &t, "Cancel").clicked() {
                        self.show_new_playlist = false;
                    }
                });
            });

        if submit {
            let name = self.new_playlist_name.clone();
            if self.create_playlist(&name) {
                self.show_new_playlist = false;
                self.new_playlist_name.clear();
                self.new_playlist_error = None;
            }
        }
        if !open {
            self.show_new_playlist = false;
        }
    }

    pub(super) fn hvsc_mirror_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let mut open = self.show_hvsc_mirror_editor;
        let mut submit = false;
        let editing = self.hvsc_mirror_editor.id.is_some();
        let title = if editing {
            "Edit HVSC mirror"
        } else {
            "Add HVSC mirror"
        };
        let error = self.hvsc_mirror_editor_error.clone();

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(20)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(
                    RichText::new(title)
                        .font(theme::semibold(16.0))
                        .color(t.text),
                );
                ui.label(
                    RichText::new(
                        "Use an HVSC directory URL, a ?dir= mirror URL, or a local C64Music folder.",
                    )
                    .font(theme::regular(11.5))
                    .color(t.text_muted),
                );
                ui.add_space(10.0);

                ui.label(
                    RichText::new("Name")
                        .font(theme::medium(12.0))
                        .color(t.text),
                );
                let name_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.hvsc_mirror_editor.name)
                        .hint_text("e.g. Local HVSC")
                        .desired_width(f32::INFINITY)
                        .font(theme::regular(13.0)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("URL or folder")
                        .font(theme::medium(12.0))
                        .color(t.text),
                );
                let location_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.hvsc_mirror_editor.location)
                        .hint_text("https://… or C:\\HVSC\\C64Music")
                        .desired_width(f32::INFINITY)
                        .font(theme::regular(13.0)),
                );
                if (name_resp.lost_focus() || location_resp.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    submit = true;
                }

                if let Some(message) = &error {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(message)
                            .font(theme::regular(11.5))
                            .color(t.accent_text),
                    );
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, &t, "Save").clicked() {
                        submit = true;
                    }
                    if secondary_button(ui, &t, "Cancel").clicked() {
                        self.show_hvsc_mirror_editor = false;
                    }
                });
            });

        if submit {
            self.save_hvsc_mirror_editor();
        }
        if !open {
            self.show_hvsc_mirror_editor = false;
            self.hvsc_mirror_editor_error = None;
        }
    }

    /// CSDb release list for the SID selected in the Now playing panel.
    pub(super) fn csdb_releases_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let Some(dialog) = self.csdb_releases.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;

        egui::Window::new("Releases Using This SID")
            .collapsible(false)
            .resizable(true)
            .default_size(vec2(640.0, 520.0))
            .min_size(vec2(420.0, 320.0))
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(18)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                let count = dialog.releases.len();
                let title = if dialog.loading || dialog.error.is_some() {
                    "Releases Using This SID".to_string()
                } else {
                    format!(
                        "{count} Release{} Using This SID",
                        if count == 1 { "" } else { "s" }
                    )
                };
                ui.label(
                    RichText::new(title)
                        .font(theme::semibold(16.0))
                        .color(t.text),
                );
                ui.label(
                    RichText::new(&dialog.sid_path)
                        .font(theme::regular(11.0))
                        .color(t.text_faint),
                );
                ui.add_space(8.0);

                if dialog.loading {
                    ui.label(
                        RichText::new("Fetching releases from CSDb…")
                            .font(theme::regular(12.0))
                            .color(t.text_muted),
                    );
                }
                if let Some(error) = &dialog.error {
                    ui.label(
                        RichText::new(error)
                            .font(theme::regular(12.0))
                            .color(t.accent_text),
                    );
                }
                if !dialog.loading && dialog.error.is_none() && count == 0 {
                    ui.label(
                        RichText::new("No releases were found for this SID.")
                            .font(theme::regular(12.0))
                            .color(t.text_muted),
                    );
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let index_width = count.to_string().len();
                        for (index, release) in dialog.releases.iter().enumerate() {
                            if index > 0 {
                                ui.separator();
                            }
                            ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                                let number = format!("{:>width$}", index + 1, width = index_width);
                                ui.label(
                                    RichText::new(number)
                                        .font(theme::semibold(18.0))
                                        .color(t.text),
                                );
                                ui.vertical(|ui| {
                                    ui.set_width(ui.available_width());
                                    let response = ui.add(egui::Link::new(
                                        RichText::new(&release.release).font(theme::medium(15.0)),
                                    ));
                                    if response.clicked() {
                                        open_url(&crate::csdb::url_for(&release.release_path));
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        if !release.author.is_empty() {
                                            ui.label(RichText::new("by").color(t.text_muted));
                                            let response = ui.add(egui::Link::new(&release.author));
                                            if response.clicked() {
                                                open_url(&crate::csdb::url_for(
                                                    &release.author_path,
                                                ));
                                            }
                                            if !release.info.is_empty() {
                                                ui.label(RichText::new(",").color(t.text_muted));
                                            }
                                        }
                                        if !release.info.is_empty() {
                                            ui.label(
                                                RichText::new(&release.info)
                                                    .font(theme::regular(11.0))
                                                    .color(t.text_muted),
                                            );
                                        }
                                    });
                                });
                            });
                        }
                    });

                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if secondary_button(ui, &t, "Close").clicked() {
                        close = true;
                    }
                });
            });

        if !open || close {
            return;
        }
        self.csdb_releases = Some(dialog);
    }

    pub(super) fn confirm_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let Some(action) = self.confirm_action else {
            return;
        };
        let mut open = true;
        let mut confirmed = false;
        let mut cancel = false;

        let playlist_name = playlist_display_name(&self.playlist_path);
        let track_name = self
            .playlist
            .entries
            .get(self.selected_playlist_idx)
            .map(|e| e.display_name.clone());
        let message = match action {
            ConfirmAction::DeletePlaylist => format!(
                "Delete playlist “{playlist_name}”? The .m3u file is removed from your playlist library. This cannot be undone."
            ),
            ConfirmAction::RemoveTrack => match track_name {
                Some(name) => {
                    format!("Remove “{name}” from playlist “{playlist_name}”?")
                }
                None => format!(
                    "Remove the selected track from playlist “{playlist_name}”?"
                ),
            },
            ConfirmAction::ClearTracks => format!(
                "Remove all {} tracks from playlist “{playlist_name}”? This cannot be undone.",
                self.playlist.len()
            ),
        };

        egui::Window::new(action.title())
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(16)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                // The caption already sits in the window title bar; repeating
                // it here would show "Delete playlist" twice.
                ui.label(
                    RichText::new(message)
                        .font(theme::regular(11.5))
                        .color(t.text_muted),
                );
                ui.add_space(12.0);
                // `Align::Min` on the cross axis keeps the button row hugging
                // its content; `Align::Center` would stretch the row over the
                // whole remaining height and leave a large gap.
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if primary_button(ui, &t, "Yes").clicked() {
                        confirmed = true;
                    }
                    if secondary_button(ui, &t, "No").clicked() {
                        cancel = true;
                    }
                });
            });

        if !open || cancel {
            self.confirm_action = None;
            return;
        }
        if confirmed {
            match action {
                ConfirmAction::DeletePlaylist => self.delete_active_playlist(),
                ConfirmAction::RemoveTrack => self.remove_playlist_track(),
                ConfirmAction::ClearTracks => self.clear_playlist_tracks(),
            }
            self.confirm_action = None;
        }
    }

    /// Reports a freshly created random playlist. The Playlist view is
    /// revealed only after the user presses "Ok", so the summary is read
    /// before the app navigates away from the Random List panel.
    pub(super) fn random_list_result_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let Some(message) = self.random_list_result.clone() else {
            return;
        };
        let mut open = true;
        let mut acknowledged = false;

        egui::Window::new("Random List created")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(16)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new("Your random playlist is ready")
                        .font(theme::semibold(16.0))
                        .color(t.text),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&message)
                        .font(theme::regular(11.5))
                        .color(t.text_muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if primary_button(ui, &t, "Ok").clicked() {
                        acknowledged = true;
                    }
                });
            });

        if acknowledged {
            self.random_list_result = None;
            self.view = View::Playlist;
        } else if !open {
            // Closing with the window button or Escape leaves the view alone.
            self.random_list_result = None;
        }
    }

    /// Reports the result of a multi-file drag-and-drop. The dialog is shown in
    /// both the full shell and the mini player; it only reports the outcome and
    /// never changes the current view.
    pub(super) fn drop_report_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let Some(report) = self.drop_report.as_ref() else {
            return;
        };
        let mut open = true;
        let mut acknowledged = false;

        egui::Window::new(report.title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(16)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new(&report.message)
                        .font(theme::regular(11.5))
                        .color(t.text_muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if primary_button(ui, &t, "Ok").clicked() {
                        acknowledged = true;
                    }
                });
            });

        if acknowledged || !open {
            self.drop_report = None;
        }
    }

    /// Progress dialog for saving every SID file of a playlist to one folder.
    pub(super) fn playlist_download_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let state = self.playlist_download.clone();
        let (title, done, total, detail, finished) = match &state {
            PlaylistDownloadProgress::Running {
                done,
                total,
                current,
                failed,
            } => {
                let failures = if *failed > 0 {
                    format!(" ({failed} failed so far)")
                } else {
                    String::new()
                };
                (
                    "Saving playlist SID files",
                    *done,
                    *total,
                    format!("Saving {current}…{failures}"),
                    false,
                )
            }
            PlaylistDownloadProgress::Finished { total, failed } => {
                let detail = if *failed == 0 {
                    format!("Saved all {total} SID files.")
                } else {
                    format!(
                        "Saved {} of {total} SID files; {failed} failed.",
                        total - failed
                    )
                };
                ("Playlist download complete", *total, *total, detail, true)
            }
            PlaylistDownloadProgress::Idle => return,
        };
        let fraction = if total == 0 {
            1.0
        } else {
            (done as f32 / total as f32).clamp(0.0, 1.0)
        };

        let mut open = true;
        let mut acknowledged = false;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(16)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(
                    RichText::new(detail)
                        .font(theme::regular(12.0))
                        .color(t.text),
                );
                ui.add_space(10.0);
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .show_percentage()
                        .desired_width(ui.available_width()),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{done} / {total} files"))
                        .font(theme::regular(11.0))
                        .color(t.text_muted),
                );
                if finished {
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if primary_button(ui, &t, "Ok").clicked() {
                            acknowledged = true;
                        }
                    });
                }
            });

        // A running job cannot be dismissed: the dialog reappears until it ends.
        if acknowledged || (!open && finished) {
            self.playlist_download = PlaylistDownloadProgress::Idle;
        }
    }

    pub(super) fn rename_playlist_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let mut open = self.show_rename_playlist;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Rename playlist")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(20)),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(
                    RichText::new("Rename playlist")
                        .font(theme::semibold(16.0))
                        .color(t.text),
                );
                ui.label(
                    RichText::new(
                        "The playlist keeps its tracks and is renamed inside its folder.",
                    )
                    .font(theme::regular(11.5))
                    .color(t.text_muted),
                );
                ui.add_space(10.0);

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_playlist_name)
                        .hint_text("Playlist name")
                        .desired_width(f32::INFINITY)
                        .font(theme::regular(13.0)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }

                if let Some(err) = &self.rename_playlist_error {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(err)
                            .font(theme::regular(11.5))
                            .color(t.accent_text),
                    );
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let can_rename = !self.rename_playlist_name.trim().is_empty();
                    if primary_button(ui, &t, "Rename").clicked() && can_rename {
                        submit = true;
                    }
                    if secondary_button(ui, &t, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if submit {
            let name = self.rename_playlist_name.clone();
            if self.rename_playlist(&name) {
                self.show_rename_playlist = false;
                self.rename_playlist_error = None;
            }
        }
        if !open || cancel {
            self.show_rename_playlist = false;
        }
    }

    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::Window::new("About")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(400.0);
                // Header: artwork on the left with the name, tagline, version
                // and author beside it. They share the text column so the block
                // reads as one unit.
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), ABOUT_HEADER),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(ABOUT_ICON), Sense::hover());
                        match crate::app_icon::texture(ctx) {
                            Some(texture) => {
                                egui::Image::from_texture(&texture)
                                    .fit_to_exact_size(icon_rect.size())
                                    .paint_at(ui, icon_rect);
                            }
                            None => {
                                // Fall back to the vector mark if the PNG cannot
                                // be decoded or uploaded.
                                ui.painter().rect_filled(
                                    icon_rect,
                                    CornerRadius::same(20),
                                    t.accent_soft,
                                );
                                paint_icon(
                                    ui.painter(),
                                    icon_rect.shrink(18.0),
                                    "music",
                                    t.accent,
                                    1.8,
                                );
                            }
                        }
                        ui.add_space(ABOUT_GAP);
                        ui.vertical(|ui| {
                            // The header block is placed by hand, so the
                            // version and author lines sit tight under the
                            // tagline instead of on the default row spacing.
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(
                                RichText::new("SidCraft")
                                    .font(theme::semibold(20.0))
                                    .color(t.text),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new("SID player and HVSC browser")
                                    .font(theme::regular(12.0))
                                    .color(t.text_muted),
                            );
                            ui.add_space(7.0);
                            ui.label(
                                RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                                    .font(theme::regular(11.5))
                                    .color(t.text_faint),
                            );
                            ui.label(
                                RichText::new("Coded by emarti, Murat Özdemir")
                                    .font(theme::regular(11.5))
                                    .color(t.text_faint),
                            );
                        });
                    },
                );
                ui.add_space(14.0);
                hairline(ui, &t);
                ui.add_space(10.0);
                let link = about_link_row(ui, &t, "Web Site", ABOUT_WEBSITE);
                if link.clicked() {
                    open_url(ABOUT_WEBSITE);
                }
                ui.add_space(10.0);
                hairline(ui, &t);
                ui.add_space(10.0);
                // The licence block: the project licence first, then the
                // third-party code the About window is expected to call out.
                egui::Frame::NONE
                    .fill(t.card)
                    .stroke(Stroke::new(1.0_f32, t.divider))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin {
                        left: 12,
                        right: 12,
                        top: 10,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Licenses")
                                .font(theme::semibold(11.5))
                                .color(t.text),
                        );
                        ui.add_space(3.0);
                        ui.label(
                            RichText::new(
                                "SidCraft is free software under the GNU General Public License \
                                 v3.0 only. It includes code from the projects below.",
                            )
                            .font(theme::regular(11.0))
                            .color(t.text_muted),
                        );
                        ui.add_space(7.0);
                        about_license_row(ui, &t, "SidCraft", "GPL-3.0-only");
                        about_license_row(ui, &t, "crabsid (Mikael Lund)", "MIT");
                        about_license_row(ui, &t, "PdfCraft theme tokens", "MIT OR Apache-2.0");
                        about_license_row(ui, &t, "residfp", "GPL-3.0-or-later");
                        ui.add_space(7.0);
                        ui.label(
                            RichText::new(
                                "Full license texts are in the licenses folder of the source \
                                 distribution.",
                            )
                            .font(theme::regular(10.0))
                            .color(t.text_faint),
                        );
                    });
                ui.add_space(14.0);
                hairline(ui, &t);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let busy = self.update_busy();
                    let clicked = ui
                        .add_enabled_ui(!busy, |ui| secondary_button(ui, &t, "Check for Updates"))
                        .inner
                        .clicked();
                    if clicked {
                        self.start_update_check(true);
                    }
                    if busy {
                        ui.spinner();
                    }
                    if let Some(line) = self.update_about_line() {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(line)
                                .font(theme::regular(11.0))
                                .color(t.text_muted),
                        );
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, &t, "Ok").clicked() {
                            self.show_about = false;
                        }
                    });
                });
            });
    }

    // ── small presentation helpers ──────────────────────────────────────────

    /// Modal update window: check result, download progress and the install
    /// step. While a download runs, Esc and clicks on the backdrop are ignored
    /// so the transfer can only be stopped with Cancel.
    pub(super) fn update_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let busy = matches!(
            self.update_phase,
            UpdatePhase::Downloading { .. } | UpdatePhase::Cancelling
        );
        let phase = self.update_phase.clone();
        let mut install_now = false;

        let modal = egui::Modal::new(egui::Id::new("sidcraft-update"))
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(
                    RichText::new("Software Update")
                        .font(theme::semibold(17.0))
                        .color(t.text),
                );
                ui.add_space(10.0);

                match phase {
                    UpdatePhase::Idle => {
                        ui.label("No update information is available.");
                    }
                    UpdatePhase::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("Checking for updates…")
                                    .font(theme::regular(12.5))
                                    .color(t.text_muted),
                            );
                        });
                    }
                    UpdatePhase::Available => {
                        let (version, notes) = self
                            .update_release
                            .as_ref()
                            .map(|release| (release.version.clone(), release.notes.clone()))
                            .unwrap_or_default();
                        ui.label(
                            RichText::new(format!("SidCraft {version} is available."))
                                .font(theme::semibold(13.5))
                                .color(t.text),
                        );
                        ui.label(
                            RichText::new(format!(
                                "You are running {}.",
                                env!("CARGO_PKG_VERSION")
                            ))
                            .font(theme::regular(11.5))
                            .color(t.text_muted),
                        );
                        if !notes.trim().is_empty() {
                            ui.add_space(10.0);
                            egui::ScrollArea::vertical()
                                .max_height(170.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(notes)
                                            .font(theme::regular(11.5))
                                            .color(t.text_muted),
                                    );
                                });
                        }
                        ui.add_space(14.0);
                        let can_download = self
                            .update_release
                            .as_ref()
                            .is_some_and(|release| release.download_url.is_some());
                        if !can_download {
                            ui.label(
                                RichText::new(
                                    "This release has no download for your platform. \
                                     Open the release page to choose one.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_faint),
                            );
                            ui.add_space(8.0);
                        }
                        ui.horizontal(|ui| {
                            if can_download
                                && primary_button(ui, &t, "Download").clicked()
                            {
                                self.start_update_download();
                            }
                            if !can_download {
                                if secondary_button(ui, &t, "Open release page").clicked()
                                    && let Some(release) = self.update_release.as_ref()
                                {
                                    open_url(&release.page_url);
                                }
                            }
                            if secondary_button(ui, &t, "Later").clicked() {
                                self.show_update_dialog = false;
                            }
                        });
                    }
                    UpdatePhase::Downloading { received, total } => {
                        let fraction = total
                            .filter(|total| *total > 0)
                            .map_or(0.0, |total| {
                                (received as f32 / total as f32).clamp(0.0, 1.0)
                            });
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .show_percentage()
                                .desired_width(ui.available_width()),
                        );
                        ui.add_space(6.0);
                        let line = match total {
                            Some(total) => format!(
                                "{} of {}",
                                crate::update::format_size(received),
                                crate::update::format_size(total)
                            ),
                            None => format!(
                                "{} downloaded",
                                crate::update::format_size(received)
                            ),
                        };
                        ui.label(
                            RichText::new(line)
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if secondary_button(ui, &t, "Cancel").clicked() {
                                self.cancel_update_download();
                            }
                        });
                    }
                    UpdatePhase::Cancelling => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("Cancelling the download…")
                                    .font(theme::regular(12.5))
                                    .color(t.text_muted),
                            );
                        });
                    }
                    UpdatePhase::Ready(path) => {
                        ui.label(
                            RichText::new("The update has been downloaded.")
                                .font(theme::semibold(13.5))
                                .color(t.text),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(path.display().to_string())
                                .font(theme::regular(10.5))
                                .color(t.text_faint),
                        );
                        ui.add_space(10.0);
                        if crate::update::can_install_in_place() {
                            ui.label(
                                RichText::new(
                                    "SidCraft will close, replace itself with the new \
                                     version and start again.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                            );
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if primary_button(ui, &t, "Restart and Install").clicked() {
                                    install_now = true;
                                    self.show_update_dialog = false;
                                }
                                if secondary_button(ui, &t, "Later").clicked() {
                                    self.show_update_dialog = false;
                                }
                            });
                        } else {
                            ui.label(
                                RichText::new(
                                    "Quit SidCraft, open the DMG and drag SidCraft into \
                                     Applications.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                            );
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if primary_button(ui, &t, "Open DMG").clicked() {
                                    let _ = crate::update::open_path(&path);
                                }
                                if secondary_button(ui, &t, "Show in Finder").clicked() {
                                    let _ = crate::update::reveal_path(&path);
                                }
                                if secondary_button(ui, &t, "Later").clicked() {
                                    self.show_update_dialog = false;
                                }
                            });
                        }
                    }
                    UpdatePhase::UpToDate => {
                        ui.label(
                            RichText::new(format!(
                                "SidCraft {} is up to date.",
                                env!("CARGO_PKG_VERSION")
                            ))
                            .font(theme::semibold(13.5))
                            .color(t.text),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No newer stable release was found on GitHub.")
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if primary_button(ui, &t, "Ok").clicked() {
                                self.show_update_dialog = false;
                            }
                        });
                    }
                    UpdatePhase::CheckFailed(error) | UpdatePhase::DownloadFailed(error) => {
                        ui.label(
                            RichText::new("The update could not be completed.")
                                .font(theme::semibold(13.5))
                                .color(t.text),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(error)
                                .font(theme::regular(11.5))
                                .color(t.accent_text),
                        );
                        ui.add_space(12.0);
                        let can_retry = self
                            .update_release
                            .as_ref()
                            .is_some_and(|release| release.download_url.is_some());
                        ui.horizontal(|ui| {
                            if can_retry && secondary_button(ui, &t, "Try Again").clicked() {
                                self.start_update_download();
                            }
                            if primary_button(ui, &t, "Close").clicked() {
                                self.show_update_dialog = false;
                            }
                        });
                    }
                }
            });

        if install_now {
            self.install_update(ctx);
        }
        if !busy && modal.should_close() {
            self.show_update_dialog = false;
        }
    }

}

