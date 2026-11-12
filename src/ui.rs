// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// The shell layout follows the PdfCraft / Craft grammar (see the PdfCraft
// project, MIT OR Apache-2.0, Copyright (c) 2026 ArtCraft Team and the
// PdfCraft contributors). This file is an independent implementation written
//! for SidCraft; no code is copied from PdfCraft. The shared design tokens live
//! in `theme.rs`, which does carry derived PdfCraft code.
//! The SidCraft desktop shell.
//!
//! The layout grammar follows the PdfCraft / Craft shell: a title/tab strip, a
//! mode bar with menus and search, a left tool panel, a central workspace on a
//! pasteboard, a right inspector plus a navigation rail, and a status bar. The
//! SID-specific views (player, playlist, HVSC, settings) live inside that shell.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Layout, Margin, Pos2, Rect,
    Response, RichText, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use residfp::ChipModel;

use crate::engine::playlist::playlist_display_name;
use crate::theme::{self, ThemeKind, ThemePreference, Tokens};
use crate::visuals::SPECTROGRAM_COLUMNS;
use crate::{ConfirmAction, RepeatMode, SidcraftApp, View, VizMode};

impl eframe::App for SidcraftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_async_hvsc();
        self.handle_seek();

        // Pull the emulated voice state once per frame for the meters and scopes.
        // `try_lock` matters while a seek is fast-forwarding in the background:
        // blocking here would freeze the whole UI.
        let viz_mode = self.viz_mode;
        if let Ok(p) = self.player.try_lock() {
            self.vu_meter.update(&p.voice_levels());
            let envelopes = p.envelope_samples();
            self.voice_scopes.update(&envelopes);
            match viz_mode {
                VizMode::Spectrum | VizMode::Spectrogram | VizMode::Oscilloscope => {
                    self.audio_samples = p.audio_samples();
                }
                VizMode::PianoRoll => {
                    self.note_history = p.note_history();
                }
                VizMode::Filter => {
                    self.voice_infos = p.voice_infos();
                    self.filter_infos = p.filter_infos();
                }
                VizMode::Adsr => {
                    self.envelope_curves = envelopes;
                    self.envelope_states = p.envelope_states();
                }
                VizMode::Scope | VizMode::Vu => {}
            }
        }

        // The FFT only runs while one of the spectrum views is visible.
        if matches!(viz_mode, VizMode::Spectrum | VizMode::Spectrogram) {
            self.spectrum.update(&self.audio_samples, self.sample_rate);
            if viz_mode == VizMode::Spectrogram {
                self.spectrogram.push(&self.spectrum.bands);
            }
        }

        // Auto-advance when the song length database says the tune is over.
        if let Some(until) = self.track_gap_until {
            if Instant::now() >= until {
                self.track_gap_until = None;
                self.paused = false;
                self.next_track();
            }
        } else {
            let elapsed = self.current_elapsed();
            if !self.paused
                && !self.seeking
                && self.song_timeout.as_secs() > 0
                && elapsed >= self.song_timeout
            {
                if self.current_song < self.total_songs {
                    self.change_subsong(self.current_song + 1);
                } else if !self.playlist.is_empty() {
                    // 1 second silence between playlist track transitions.
                    self.track_gap_until = Some(Instant::now() + Duration::from_secs(1));
                    self.paused = true;
                    self.song_elapsed = self.song_timeout;
                    if let Ok(mut p) = self.player.lock() {
                        if !p.is_paused() {
                            p.toggle_pause();
                        }
                    }
                } else {
                    // No playlist: keep previous behavior.
                    self.next_track();
                }
            }
        }

        // Resolve the saved system/light/dark preference and install the same
        // design tokens and egui style as the rest of the Craft desktop suite.
        let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark);
        let kind = self.theme_pref.resolve(system_dark, ThemeKind::Light);
        if self.applied_theme != Some(kind) {
            theme::apply(ctx, kind);
            self.applied_theme = Some(kind);
        } else {
            Tokens::store(ctx, Tokens::for_kind(kind));
        }

        self.keyboard_shortcuts(ctx);
        self.sync_config();
        self.remember_window(ctx);

        self.tab_strip(ctx);
        self.mode_bar(ctx);
        self.status_bar(ctx);
        self.right_rail(ctx);
        if self.right_open {
            self.right_panel(ctx);
        }
        if self.left_open {
            self.left_panel(ctx);
        }
        self.central(ctx);
        if self.show_about {
            self.about_window(ctx);
        }
        if self.show_new_playlist {
            self.new_playlist_window(ctx);
        }
        if self.show_rename_playlist {
            self.rename_playlist_window(ctx);
        }
        if self.confirm_action.is_some() {
            self.confirm_window(ctx);
        }

        // Repaint only as fast as needed: real-time visuals/timing while playing
        // or seeking, a precise wake-up for the inter-track gap, otherwise idle.
        if let Some(until) = self.track_gap_until {
            let wait = until.saturating_duration_since(Instant::now());
            ctx.request_repaint_after(wait);
        } else if (!self.paused && self.sid_file.name.is_empty() == false) || self.seeking {
            // ~40 FPS for smooth meters, scopes and elapsed time while playing.
            ctx.request_repaint_after(Duration::from_millis(25));
        } else if self.hvsc_loading {
            // Keep the loading state responsive without running at full rate.
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.save();
    }
}

impl SidcraftApp {
    // ── keyboard ────────────────────────────────────────────────────────────

    fn keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_pause();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) && i.modifiers.shift) {
            self.next_track();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft) && i.modifiers.shift) {
            self.prev_track();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.command) {
            self.open_file_dialog();
        }
    }

    // ── tab strip ───────────────────────────────────────────────────────────

    fn tab_strip(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::TopBottomPanel::top("sidcraft_tab_strip")
            .exact_height(38.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.titlebar)
                    .inner_margin(Margin {
                        left: 8,
                        right: 10,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if icon_button(ui, &t, "home", 28.0, self.view == View::Home, "Home")
                        .clicked()
                    {
                        self.view = View::Home;
                    }

                    let title = if self.sid_file.name.is_empty() {
                        "SID Player".to_string()
                    } else {
                        self.sid_file.name.clone()
                    };
                    let selected = self.view != View::Home;
                    if document_tab(ui, &t, &title, selected).clicked() {
                        self.view = if self.last_view == View::Home {
                            View::Player
                        } else {
                            self.last_view
                        };
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let theme_tip = if t.dark() {
                            "Switch to light theme"
                        } else {
                            "Switch to dark theme"
                        };
                        let (theme_rect, theme_resp) =
                            ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                        let theme_bg = if theme_resp.hovered() {
                            t.hover
                        } else {
                            t.titlebar
                        };
                        if theme_resp.hovered() {
                            ui.painter().rect_filled(
                                theme_rect,
                                CornerRadius::same(6),
                                theme_bg,
                            );
                        }
                        paint_theme_icon(
                            ui.painter(),
                            theme_rect.shrink(6.0),
                            t.dark(),
                            if theme_resp.hovered() { t.accent } else { t.text },
                            theme_bg,
                        );
                        if theme::hand(theme_resp).on_hover_text(theme_tip).clicked() {
                            self.theme_pref = if t.dark() {
                                ThemePreference::Light
                            } else {
                                ThemePreference::Dark
                            };
                        }
                        if icon_button(ui, &t, "info", 28.0, false, "About SidCraft").clicked() {
                            self.show_about = true;
                        }
                        let open = toolbar_button(ui, &t, "folder", "Open", false);
                        if open.clicked() {
                            self.open_file_dialog();
                        }
                    });
                });
            });
    }

    // ── mode bar ────────────────────────────────────────────────────────────

    fn mode_bar(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::TopBottomPanel::top("sidcraft_mode_bar")
            .exact_height(48.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.chrome)
                    .inner_margin(Margin::symmetric(10, 0))
                    .stroke(Stroke::new(1.0_f32, t.divider)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    let menu_resp = ui.menu_button(
                        RichText::new("☰  Menu")
                            .font(theme::medium(13.0))
                            .color(t.text),
                        |ui| self.menus(ui),
                    );
                    if menu_resp.response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    ui.add_space(6.0);
                    let sep = ui.cursor().left();
                    ui.painter().vline(
                        sep,
                        ui.max_rect().y_range().shrink(12.0),
                        Stroke::new(1.0_f32, t.divider),
                    );
                    ui.add_space(10.0);

                    for view in [View::Player, View::Playlist, View::Hvsc, View::Settings] {
                        if mode_tab(ui, &t, view.label(), self.view == view).clicked() {
                            self.view = view;
                            self.last_view = view;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if icon_button(ui, &t, "panel-right", 32.0, self.right_open, "Toggle inspector")
                            .clicked()
                        {
                            self.right_open = !self.right_open;
                        }
                        if icon_button(ui, &t, "panel-left", 32.0, self.left_open, "Toggle tool panel")
                            .clicked()
                        {
                            self.left_open = !self.left_open;
                        }
                        ui.add_space(4.0);
                        let search = search_box(
                            ui,
                            &t,
                            &mut self.global_search,
                            240.0,
                            "Search HVSC tunes",
                        );
                        if search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.hvsc_search_query = self.global_search.clone();
                            self.view = View::Hvsc;
                            self.last_view = View::Hvsc;
                            self.start_hvsc_search();
                        }
                    });
                });
            });
    }

    fn menus(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            if ui.button("Open SID or playlist…").clicked() {
                ui.close_menu();
                self.open_file_dialog();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.close_menu();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        ui.menu_button("Playback", |ui| {
            let pause_label = if self.paused { "Play" } else { "Pause" };
            if ui.button(pause_label).clicked() {
                ui.close_menu();
                self.toggle_pause();
            }
            if ui.button("Stop").clicked() {
                ui.close_menu();
                self.stop_playback();
            }
            ui.separator();
            if ui.button("Previous track").clicked() {
                ui.close_menu();
                self.prev_track();
            }
            if ui.button("Next track").clicked() {
                ui.close_menu();
                self.next_track();
            }
            ui.separator();
            let can_prev = self.current_song > 1;
            let can_next = self.current_song < self.total_songs;
            if ui.add_enabled(can_prev, egui::Button::new("Previous subsong")).clicked() {
                ui.close_menu();
                self.change_subsong(self.current_song - 1);
            }
            if ui.add_enabled(can_next, egui::Button::new("Next subsong")).clicked() {
                ui.close_menu();
                self.change_subsong(self.current_song + 1);
            }
            ui.separator();
            if ui.button("Cycle SID chip (6581 / 8580)").clicked() {
                ui.close_menu();
                self.cycle_chip_model();
            }
            let mut ekv = self.ekv_filter;
            if ui.checkbox(&mut ekv, "EKV transistor filter").changed() {
                self.set_ekv_filter(ekv);
            }
        });

        ui.menu_button("View", |ui| {
            for view in [
                View::Home,
                View::Player,
                View::Playlist,
                View::Hvsc,
                View::Settings,
            ] {
                if ui
                    .add(egui::Button::new(view.label()).selected(self.view == view))
                    .clicked()
                {
                    ui.close_menu();
                    self.view = view;
                    if view != View::Home {
                        self.last_view = view;
                    }
                }
            }
            ui.separator();
            let mut left = self.left_open;
            if ui.checkbox(&mut left, "Tool panel").changed() {
                self.left_open = left;
            }
            let mut right = self.right_open;
            if ui.checkbox(&mut right, "Inspector").changed() {
                self.right_open = right;
            }
            ui.separator();
            for pref in [
                ThemePreference::System,
                ThemePreference::Light,
                ThemePreference::Dark,
            ] {
                if ui
                    .add(
                        egui::Button::new(format!("{} theme", pref.label()))
                            .selected(self.theme_pref == pref),
                    )
                    .clicked()
                {
                    self.theme_pref = pref;
                }
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About SidCraft").clicked() {
                ui.close_menu();
                self.show_about = true;
            }
            if ui.button("Open HVSC browser").clicked() {
                ui.close_menu();
                self.view = View::Hvsc;
                self.last_view = View::Hvsc;
            }
        });
    }

    // ── left tool panel ─────────────────────────────────────────────────────

    fn left_panel(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::SidePanel::left("sidcraft_tools")
            .resizable(true)
            .default_width(244.0)
            .min_width(204.0)
            .max_width(340.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 12,
                        bottom: 10,
                    })
                    .stroke(Stroke::new(1.0_f32, t.divider)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let title_w = (ui.available_width() - 26.0 - ui.spacing().item_spacing.x).max(1.0);
                    ui.add_sized(
                        [title_w, 26.0],
                        egui::Label::new(
                            RichText::new("Tools")
                                .font(theme::semibold(15.5))
                                .color(t.text),
                        ),
                    );
                    if icon_button(ui, &t, "x", 26.0, false, "Close panel").clicked() {
                        self.left_open = false;
                    }
                });
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(6.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width((ui.available_width() - 14.0).max(0.0));
                        ui.add_space(2.0);

                        if tool_row(ui, &t, "home", "Home", "Overview and recent tunes", self.view == View::Home).clicked() {
                            self.view = View::Home;
                        }
                        if tool_row(ui, &t, "music", "Player", "Now playing and visualiser", self.view == View::Player).clicked() {
                            self.view = View::Player;
                            self.last_view = View::Player;
                        }
                        if tool_row(ui, &t, "list", "Playlist", &format!("{} tracks", self.playlist.len()), self.view == View::Playlist).clicked() {
                            self.view = View::Playlist;
                            self.last_view = View::Playlist;
                        }
                        if tool_row(ui, &t, "globe", "HVSC browser", "Search the High Voltage SID Collection", self.view == View::Hvsc).clicked() {
                            self.view = View::Hvsc;
                            self.last_view = View::Hvsc;
                        }
                        if tool_row(ui, &t, "gear", "Settings", "Appearance, audio and emulation", self.view == View::Settings).clicked() {
                            self.view = View::Settings;
                            self.last_view = View::Settings;
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(
                            RichText::new("NOW PLAYING")
                                .font(theme::semibold(10.5))
                                .color(t.text_faint),
                        );
                        ui.add_space(4.0);
                        let title = self.track_title();
                        ui.label(
                            RichText::new(title)
                                .font(theme::semibold(13.0))
                                .color(t.text),
                        );
                        let subtitle = if self.sid_file.author.is_empty() {
                            "Unknown author".to_string()
                        } else {
                            self.sid_file.author.clone()
                        };
                        ui.label(RichText::new(subtitle).font(theme::regular(11.5)).color(t.text_muted));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(if self.paused { "▶  Play" } else { "⏸  Pause" })
                                            .font(theme::medium(12.0)),
                                    )
                                    .fill(t.accent)
                                    .stroke(Stroke::NONE)
                                    .corner_radius(CornerRadius::same(6)),
                                )
                                .clicked()
                            {
                                self.toggle_pause();
                            }
                            let open = ui.button("Open");
                            if open.clicked() {
                                self.open_file_dialog();
                            }
                            // Match the height of the neighbouring "Open"
                            // button so the row stays level.
                            let height = open.rect.height();
                            if icon_button_glyph(
                                ui,
                                &t,
                                "plus",
                                height,
                                (height * 0.8).min(21.0),
                                1.6,
                                false,
                                "Add current track to playlist",
                            )
                            .clicked()
                            {
                                self.add_current_to_playlist();
                            }
                        });
                    });
            });
    }

    // ── right rail ──────────────────────────────────────────────────────────

    fn right_rail(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::SidePanel::right("sidcraft_rail")
            .exact_width(48.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.chrome)
                    .inner_margin(Margin::symmetric(1, 8))
                    .stroke(Stroke::new(1.0_f32, t.divider)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    if icon_button(ui, &t, "home", 34.0, self.view == View::Home, "Home").clicked() {
                        self.view = View::Home;
                    }
                    if icon_button(ui, &t, "music", 34.0, self.view == View::Player, "Player").clicked() {
                        self.view = View::Player;
                        self.last_view = View::Player;
                    }
                    if icon_button(ui, &t, "list", 34.0, self.view == View::Playlist, "Playlist").clicked() {
                        self.view = View::Playlist;
                        self.last_view = View::Playlist;
                    }
                    if icon_button(ui, &t, "globe", 34.0, self.view == View::Hvsc, "HVSC browser").clicked() {
                        self.view = View::Hvsc;
                        self.last_view = View::Hvsc;
                    }
                    if icon_button(ui, &t, "gear", 34.0, self.view == View::Settings, "Settings").clicked() {
                        self.view = View::Settings;
                        self.last_view = View::Settings;
                    }
                });

                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    if icon_button(ui, &t, "panel-right", 32.0, self.right_open, "Toggle inspector").clicked() {
                        self.right_open = !self.right_open;
                    }
                    ui.add_space(4.0);
                    if icon_button(ui, &t, "next", 32.0, false, "Next track").clicked() {
                        self.next_track();
                    }
                    if icon_button(ui, &t, if self.paused { "play" } else { "pause" }, 32.0, false, "Play / pause").clicked() {
                        self.toggle_pause();
                    }
                    if icon_button(ui, &t, "prev", 32.0, false, "Previous track").clicked() {
                        self.prev_track();
                    }
                });
            });
    }

    // ── right inspector ─────────────────────────────────────────────────────

    fn right_panel(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::SidePanel::right("sidcraft_inspector")
            .resizable(true)
            .default_width(300.0)
            .min_width(250.0)
            .max_width(430.0)
            .frame(egui::Frame::NONE.fill(t.panel))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 12,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let title_w = (ui.available_width() - 26.0 - ui.spacing().item_spacing.x).max(1.0);
                            ui.add_sized(
                                [title_w, 26.0],
                                egui::Label::new(
                                    RichText::new("Now playing")
                                        .font(theme::semibold(15.5))
                                        .color(t.text),
                                ),
                            );
                            if icon_button(ui, &t, "x", 26.0, false, "Close panel").clicked() {
                                self.right_open = false;
                            }
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(6.0);
                    });

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::NONE
                            .inner_margin(Margin {
                                left: 14,
                                right: 14,
                                top: 2,
                                bottom: 16,
                            })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                let title = self.track_title();
                                ui.label(RichText::new(title).font(theme::semibold(17.0)).color(t.text));
                                let author = if self.sid_file.author.is_empty() {
                                    "Unknown author".to_string()
                                } else {
                                    self.sid_file.author.clone()
                                };
                                ui.label(RichText::new(author).font(theme::regular(12.5)).color(t.text_muted));
                                if !self.sid_file.released.is_empty() {
                                    ui.label(
                                        RichText::new(&self.sid_file.released)
                                            .font(theme::regular(11.5))
                                            .color(t.text_faint),
                                    );
                                }

                                ui.add_space(10.0);
                                ui.horizontal_wrapped(|ui| {
                                    badge(ui, self.clock_label(), t.accent_soft, t.accent_text);
                                    badge(ui, self.chip_label(), t.hover, t.text_muted);
                                    badge(ui, self.sid_count_label(), t.hover, t.text_muted);
                                    badge(
                                        ui,
                                        &format!("{}/{}", self.current_song, self.total_songs),
                                        t.hover,
                                        t.text_muted,
                                    );
                                });

                                ui.add_space(16.0);
                                section_title(ui, &t, "Properties");
                                property_row_right(ui, &t, "Format", &self.sid_file.magic);
                                property_row_right(ui, &t, "Songs", &self.total_songs.to_string());
                                property_row_right(ui, &t, "Subsong", &format!("{} of {}", self.current_song, self.total_songs));
                                property_row_right(ui, &t, "Clock", self.clock_label());
                                property_row_right(ui, &t, "SID chips", self.sid_count_label());
                                property_row_right(ui, &t, "Sample rate", &format!("{} Hz", self.sample_rate));
                                property_row_right(ui, &t, "Filter", if self.ekv_filter { "EKV on" } else { "Standard" });

                                ui.add_space(16.0);
                                section_title(ui, &t, "Voices");
                                ui.add_space(4.0);
                                voice_meters(ui, &t, &self.vu_meter.levels, &self.vu_meter.peaks);
                            });
                    });
            });
    }

    // ── status bar ──────────────────────────────────────────────────────────

    fn status_bar(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::TopBottomPanel::bottom("sidcraft_status")
            .exact_height(26.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.chrome)
                    .inner_margin(Margin::symmetric(10, 0))
                    .stroke(Stroke::new(1.0_f32, t.divider)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let status = if self.status_timer.elapsed().as_secs() < 4 {
                        self.status_message.clone()
                    } else {
                        "Ready".to_string()
                    };
                    ui.label(
                        RichText::new(status)
                            .font(theme::regular(11.5))
                            .color(t.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  {}  ·  {}  ·  {} Hz",
                                self.clock_label(),
                                self.chip_label(),
                                self.sid_count_label(),
                                self.sample_rate
                            ))
                            .font(theme::mono(10.5))
                            .color(t.text_faint),
                        );
                    });
                });
            });
    }

    // ── central workspace ───────────────────────────────────────────────────

    fn central(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let view = self.view;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(t.pasteboard))
            .show(ctx, |ui| match view {
                View::Home => self.home_view(ui),
                View::Player => self.player_view(ui),
                View::Playlist => self.playlist_view(ui),
                View::Hvsc => self.hvsc_view(ui),
                View::Settings => self.settings_view(ui),
            });
    }

    fn home_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(Margin {
                        left: 36,
                        right: 36,
                        top: 28,
                        bottom: 28,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Welcome to SidCraft")
                                .font(theme::semibold(24.0))
                                .color(t.text),
                        );
                        ui.label(
                            RichText::new(
                                "A local, open-source C64 SID player and HVSC browser.",
                            )
                            .font(theme::regular(14.0))
                            .color(t.text_muted),
                        );
                        ui.add_space(16.0);

                        // Now-playing card.
                        egui::Frame::NONE
                            .fill(t.card)
                            .stroke(Stroke::new(1.0_f32, t.border))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::same(16))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let (icon_rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                                    ui.painter().rect_filled(icon_rect, CornerRadius::same(10), t.accent_soft);
                                    paint_icon(ui.painter(), icon_rect.shrink(9.0), "music", t.accent, 1.7);
                                    ui.add_space(4.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(self.track_title())
                                                .font(theme::semibold(16.0))
                                                .color(t.text),
                                        );
                                        let subtitle = if self.sid_file.author.is_empty() {
                                            "Select a tune to start playing".to_string()
                                        } else {
                                            format!(
                                                "{}  ·  {}  ·  {}",
                                                self.sid_file.author,
                                                self.sid_file.released,
                                                self.clock_label()
                                            )
                                        };
                                        ui.label(
                                            RichText::new(subtitle)
                                                .font(theme::regular(12.0))
                                                .color(t.text_muted),
                                        );
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if primary_button(ui, &t, if self.paused { "Play" } else { "Pause" }).clicked() {
                                            self.toggle_pause();
                                        }
                                        if secondary_button(ui, &t, "Open file").clicked() {
                                            self.open_file_dialog();
                                        }
                                    });
                                });
                                ui.add_space(12.0);
                                let total = self.song_timeout.as_secs_f32().max(1.0);
                                let fraction = (self.current_elapsed().as_secs_f32() / total).clamp(0.0, 1.0);
                                if let Some(f) = seek_bar(ui, &t, fraction) {
                                    self.seek_to_fraction(f);
                                }
                            });

                        ui.add_space(24.0);
                        ui.label(
                            RichText::new("Quick actions")
                                .font(theme::semibold(17.0))
                                .color(t.text),
                        );
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                            let cards = [
                                (View::Player, "music", "Player", "Watch the voices and control playback"),
                                (View::Playlist, "list", "Playlist", "Open, reorder and play your m3u tracks"),
                                (View::Hvsc, "globe", "HVSC browser", "Search the High Voltage SID Collection"),
                                (View::Settings, "gear", "Settings", "Theme, emulation and visualiser"),
                            ];
                            for (view, icon, title, blurb) in cards {
                                if tool_card(ui, &t, icon, title, blurb, t.voice(view as usize)).clicked() {
                                    self.view = view;
                                    self.last_view = view;
                                }
                            }
                        });

                        ui.add_space(26.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Playlist")
                                    .font(theme::semibold(17.0))
                                    .color(t.text),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ghost_button(ui, &t, "View all").clicked() {
                                    self.view = View::Playlist;
                                    self.last_view = View::Playlist;
                                }
                            });
                        });
                        ui.add_space(8.0);
                        if self.playlist.is_empty() {
                            ui.label(
                                RichText::new("Open a SID file or add one to the playlist.")
                                    .font(theme::regular(12.5))
                                    .color(t.text_muted),
                            );
                        } else {
                            let mut play = None;
                            let rows: Vec<(usize, String, String, Option<u16>)> = self
                                .playlist
                                .entries
                                .iter()
                                .enumerate()
                                .take(7)
                                .map(|(i, e)| (i, e.display_name.clone(), e.source.clone(), e.subsong))
                                .collect();
                            for (idx, name, source, subsong) in rows {
                                let selected = idx == self.selected_playlist_idx;
                                let row = playlist_row(ui, &t, idx, &name, &source, subsong, selected);
                                if row.double_clicked() {
                                    play = Some(idx);
                                }
                            }
                            if let Some(idx) = play {
                                self.play_playlist_index(idx);
                                self.view = View::Player;
                                self.last_view = View::Player;
                            }
                        }
                    });
            });
    }

    fn player_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(24.0);
                let page_width = (ui.available_width() - 44.0).clamp(220.0, 720.0);
                ui.vertical_centered(|ui| {
                    egui::Frame::NONE
                        .fill(t.card)
                        .stroke(Stroke::new(1.0_f32, t.border))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 3],
                            blur: 14,
                            spread: 0,
                            color: t.page_shadow,
                        })
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::same(20))
                        .show(ui, |ui| {
                            ui.set_width(page_width);

                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(self.track_title())
                                            .font(theme::semibold(20.0))
                                            .color(t.text),
                                    );
                                    let author = if self.sid_file.author.is_empty() {
                                        "Unknown author".to_string()
                                    } else {
                                        self.sid_file.author.clone()
                                    };
                                    ui.label(
                                        RichText::new(format!(
                                            "{author}  ·  {}",
                                            if self.sid_file.released.is_empty() {
                                                "Unknown release"
                                            } else {
                                                &self.sid_file.released
                                            }
                                        ))
                                        .font(theme::regular(12.0))
                                        .color(t.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let mut mode = self.viz_mode;
                                    egui::ComboBox::from_id_salt("player-viz")
                                        .width(128.0)
                                        .selected_text(mode.label())
                                        .show_ui(ui, |ui| {
                                            for option in VizMode::ALL {
                                                ui.selectable_value(
                                                    &mut mode,
                                                    option,
                                                    option.label(),
                                                );
                                            }
                                        });
                                    if mode != self.viz_mode {
                                        self.viz_mode = mode;
                                    }
                                    if secondary_button(ui, &t, self.chip_label()).clicked() {
                                        self.cycle_chip_model();
                                    }
                                });
                            });

                            ui.add_space(14.0);

                            let (display_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 276.0), Sense::hover());
                            match self.viz_mode {
                                VizMode::Scope => self.paint_scope(ui, &t, display_rect),
                                VizMode::Vu => self.paint_display_vu(ui, &t, display_rect),
                                VizMode::Spectrum => self.paint_spectrum(ui, &t, display_rect),
                                VizMode::Spectrogram => {
                                    self.paint_spectrogram(ui, &t, display_rect);
                                }
                                VizMode::Oscilloscope => {
                                    self.paint_oscilloscope(ui, &t, display_rect);
                                }
                                VizMode::PianoRoll => self.paint_piano_roll(ui, &t, display_rect),
                                VizMode::Filter => self.paint_filter(ui, &t, display_rect),
                                VizMode::Adsr => self.paint_adsr(ui, &t, display_rect),
                            }

                            ui.add_space(10.0);
                            let total = self.song_timeout.as_secs_f32().max(1.0);
                            let fraction = (self.current_elapsed().as_secs_f32() / total).clamp(0.0, 1.0);
                            if let Some(f) = seek_bar(ui, &t, fraction) {
                                self.seek_to_fraction(f);
                            }
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format_duration(self.current_elapsed()))
                                        .font(theme::mono(11.0))
                                        .color(t.text_muted),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format_duration(self.song_timeout))
                                            .font(theme::mono(11.0))
                                            .color(t.text_faint),
                                    );
                                });
                            });

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if transport_button(ui, &t, "prev", 46.0, false, "Previous track").clicked() {
                                    self.prev_track();
                                }
                                if transport_button(ui, &t, if self.paused { "play" } else { "pause" }, 46.0, true, "Play / pause").clicked() {
                                    self.toggle_pause();
                                }
                                if transport_button(ui, &t, "stop", 46.0, false, "Stop").clicked() {
                                    self.stop_playback();
                                }
                                if transport_button(ui, &t, "next", 46.0, false, "Next track").clicked() {
                                    self.next_track();
                                }

                                ui.add_space(8.0);
                                if icon_button(ui, &t, "shuffle", 34.0, self.shuffle, "Shuffle").clicked() {
                                    self.shuffle = !self.shuffle;
                                }
                                if icon_button(ui, &t, "repeat", 34.0, self.repeat_mode != RepeatMode::Off, "Repeat").clicked() {
                                    self.cycle_repeat();
                                }

                                ui.add_space(8.0);
                                let sep = ui.cursor().left();
                                ui.painter().vline(sep, ui.max_rect().y_range().shrink(6.0), Stroke::new(1.0_f32, t.divider));
                                ui.add_space(8.0);

                                if icon_button(ui, &t, "chevron-left", 34.0, false, "Previous subsong").clicked() && self.current_song > 1 {
                                    self.change_subsong(self.current_song - 1);
                                }
                                ui.label(
                                    RichText::new(format!("{:02}/{:02}", self.current_song, self.total_songs))
                                        .font(theme::mono(12.0))
                                        .color(t.text_muted),
                                );
                                if icon_button(ui, &t, "chevron-right", 34.0, false, "Next subsong").clicked() && self.current_song < self.total_songs {
                                    self.change_subsong(self.current_song + 1);
                                }

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if primary_button(ui, &t, "Open").clicked() {
                                        self.open_file_dialog();
                                    }
                                    // Sized to match the prev/next transport
                                    // arrows: same glyph box and stroke.
                                    if icon_button_glyph(
                                        ui,
                                        &t,
                                        "plus",
                                        40.0,
                                        21.0,
                                        1.6,
                                        false,
                                        "Add current track to playlist",
                                    )
                                    .clicked()
                                    {
                                        self.add_current_to_playlist();
                                    }
                                    // Kept in the same right-to-left row (no nested
                                    // horizontal) so the speaker lines up with the
                                    // transport arrows.
                                    let muted =
                                        self.muted.load(std::sync::atomic::Ordering::SeqCst);
                                    if icon_button_glyph(
                                        ui,
                                        &t,
                                        if muted { "speaker-off" } else { "speaker" },
                                        40.0,
                                        21.0,
                                        1.6,
                                        false,
                                        "Mute / unmute",
                                    )
                                    .clicked()
                                    {
                                        self.muted
                                            .store(!muted, std::sync::atomic::Ordering::SeqCst);
                                    }
                                    let frac = if muted {
                                        0.0
                                    } else {
                                        *self
                                            .volume
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner())
                                    };
                                    if let Some(f) = volume_bar(ui, &t, frac, 110.0) {
                                        *self
                                            .volume
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner()) = f;
                                        // Adjusting the slider always leaves mute.
                                        self.muted.store(
                                            false,
                                            std::sync::atomic::Ordering::SeqCst,
                                        );
                                    }
                                });
                            });
                        });
                });
                ui.add_space(24.0);
            });
    }

    /// Shared LCD chrome: dark panel, status strip and bottom ticker.
    /// Returns the content rectangle the visualisers draw in.
    fn display_chrome(&self, ui: &egui::Ui, rect: Rect) -> Rect {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::same(10),
            Color32::from_rgb(0x0F, 0x10, 0x14),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.0_f32, Color32::from_white_alpha(18)),
            StrokeKind::Inside,
        );

        painter.rect_filled(
            Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 40.0)),
            CornerRadius {
                nw: 10,
                ne: 10,
                sw: 0,
                se: 0,
            },
            Color32::from_black_alpha(70),
        );
        painter.text(
            pos2(rect.left() + 14.0, rect.top() + 7.0),
            Align2::LEFT_TOP,
            format_duration(self.current_elapsed()),
            theme::mono(27.0),
            Color32::WHITE,
        );
        painter.text(
            pos2(rect.right() - 14.0, rect.top() + 13.0),
            Align2::RIGHT_TOP,
            format!(
                "{}  ·  {}  ·  {}",
                self.clock_label(),
                self.chip_label(),
                self.sid_count_label()
            ),
            theme::mono(10.0),
            Color32::from_white_alpha(170),
        );

        let ticker = Rect::from_min_max(
            pos2(rect.left() + 12.0, rect.bottom() - 26.0),
            pos2(rect.right() - 12.0, rect.bottom() - 8.0),
        );
        painter.rect_filled(ticker, CornerRadius::same(4), Color32::from_white_alpha(10));
        painter.text(
            pos2(ticker.left() + 8.0, ticker.center().y),
            Align2::LEFT_CENTER,
            format!("{} — {}", self.track_title(), self.sid_file.author),
            theme::mono(9.5),
            Color32::from_white_alpha(190),
        );

        Rect::from_min_max(rect.min + vec2(14.0, 46.0), rect.max - vec2(14.0, 34.0))
    }

    fn paint_scope(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();
        for i in 1..4 {
            let y = inner.top() + inner.height() * i as f32 / 4.0;
            painter.line_segment(
                [pos2(inner.left(), y), pos2(inner.right(), y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(8)),
            );
        }
        for i in 1..8 {
            let x = inner.left() + inner.width() * i as f32 / 8.0;
            painter.line_segment(
                [pos2(x, inner.top()), pos2(x, inner.bottom())],
                Stroke::new(1.0_f32, Color32::from_white_alpha(6)),
            );
        }

        let voices = self.voice_scopes.voice_count().max(1);
        let row_h = inner.height() / voices as f32;
        for (idx, samples) in self.voice_scopes.samples.iter().enumerate() {
            let color = t.voice(idx);
            let base_y = inner.top() + row_h * (idx as f32 + 0.5);
            painter.line_segment(
                [pos2(inner.left(), base_y), pos2(inner.right(), base_y)],
                Stroke::new(1.0_f32, color.gamma_multiply(0.16)),
            );
            let count = samples.len().min(inner.width().max(2.0) as usize).max(2);
            let mut points = Vec::with_capacity(count);
            for (i, &sample) in samples.iter().take(count).enumerate() {
                let x = inner.left() + (i as f32 / (count - 1) as f32) * inner.width();
                let y = base_y - (sample.clamp(0.0, 1.0) - 0.5) * (row_h * 0.78);
                points.push(pos2(x, y));
            }
            painter.add(egui::Shape::line(points, Stroke::new(1.4_f32, color)));
            painter.text(
                pos2(inner.left() + 2.0, base_y),
                Align2::LEFT_CENTER,
                format!("V{}", idx + 1),
                theme::mono(8.0),
                color.gamma_multiply(0.75),
            );
        }
    }

    fn paint_display_vu(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();
        let bars = Rect::from_min_max(inner.min, pos2(inner.right(), inner.bottom() - 10.0));
        let count = self.vu_meter.voice_count().max(1);
        let gap = 6.0;
        let bar_w = ((bars.width() - gap * (count.saturating_sub(1)) as f32) / count as f32).max(4.0);
        for i in 0..count {
            let level = self.vu_meter.levels.get(i).copied().unwrap_or(0.0);
            let peak = self.vu_meter.peaks.get(i).copied().unwrap_or(0.0);
            let color = t.voice(i);
            let x = bars.left() + i as f32 * (bar_w + gap);
            let track = Rect::from_min_max(pos2(x, bars.top()), pos2(x + bar_w, bars.bottom()));
            painter.rect_filled(track, CornerRadius::same(3), Color32::from_white_alpha(12));
            let h = bars.height() * level.clamp(0.0, 1.0);
            let bar = Rect::from_min_max(pos2(x, bars.bottom() - h), pos2(x + bar_w, bars.bottom()));
            painter.rect_filled(bar, CornerRadius::same(3), color);
            let peak_y = bars.bottom() - bars.height() * peak.clamp(0.0, 1.0);
            painter.line_segment(
                [pos2(x, peak_y), pos2(x + bar_w, peak_y)],
                Stroke::new(1.5_f32, Color32::from_white_alpha(210)),
            );
            painter.text(
                pos2(x + bar_w / 2.0, bars.bottom() + 4.0),
                Align2::CENTER_TOP,
                format!("V{}", i + 1),
                theme::mono(8.5),
                color.gamma_multiply(0.8),
            );
        }
    }

    fn paint_spectrum(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();

        for step in 1..4 {
            let y = inner.bottom() - inner.height() * step as f32 / 4.0;
            painter.line_segment(
                [pos2(inner.left(), y), pos2(inner.right(), y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(8)),
            );
            painter.text(
                pos2(inner.left() + 2.0, y),
                Align2::LEFT_BOTTOM,
                format!("{} dB", -16 * step),
                theme::mono(7.5),
                Color32::from_white_alpha(70),
            );
        }

        let nyquist = self.sample_rate as f32 * 0.5;
        let f_max = nyquist.min(16_000.0).max(60.0);
        let f_min = 30.0_f32.min(f_max * 0.5);
        for (freq, label) in [(100.0_f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
            if freq >= f_max {
                continue;
            }
            let frac = (freq / f_min).ln() / (f_max / f_min).ln();
            let x = inner.left() + inner.width() * frac;
            painter.line_segment(
                [pos2(x, inner.top()), pos2(x, inner.bottom())],
                Stroke::new(1.0_f32, Color32::from_white_alpha(7)),
            );
            painter.text(
                pos2(x + 2.0, inner.bottom() - 2.0),
                Align2::LEFT_BOTTOM,
                label,
                theme::mono(7.5),
                Color32::from_white_alpha(80),
            );
        }

        let count = self.spectrum.bands.len();
        if count == 0 {
            return;
        }
        let bar_w = inner.width() / count as f32;
        for (i, &value) in self.spectrum.bands.iter().enumerate() {
            let x = inner.left() + i as f32 * bar_w;
            let h = inner.height() * value.clamp(0.0, 1.0);
            let bar = Rect::from_min_max(
                pos2(x, inner.bottom() - h),
                pos2((x + bar_w - 1.0).max(x + 1.0), inner.bottom()),
            );
            painter.rect_filled(bar, CornerRadius::same(1), spectrum_color(t, value));
            let peak = self.spectrum.peaks[i].clamp(0.0, 1.0);
            let peak_y = inner.bottom() - inner.height() * peak;
            painter.line_segment(
                [pos2(x, peak_y), pos2((x + bar_w - 1.0).max(x + 1.0), peak_y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(170)),
            );
        }
    }

    fn paint_spectrogram(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();
        let columns = self.spectrogram.columns.len();
        if columns == 0 {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "Listening…",
                theme::mono(10.0),
                Color32::from_white_alpha(120),
            );
            return;
        }

        let col_w = inner.width() / SPECTROGRAM_COLUMNS as f32;
        let bands = self.spectrogram.columns.front().map_or(1, Vec::len).max(1);
        let step = (bands / 48).max(1);
        let rows = bands.div_ceil(step);
        let cell_h = inner.height() / rows as f32;
        let x_origin = inner.right() - columns as f32 * col_w;
        let mut mesh = egui::Mesh::default();
        for (c, column) in self.spectrogram.columns.iter().enumerate() {
            let x0 = x_origin + c as f32 * col_w;
            if x0 + col_w < inner.left() {
                continue;
            }
            for row in 0..rows {
                let lo = row * step;
                let hi = (lo + step).min(column.len());
                if lo >= hi {
                    break;
                }
                let value = column[lo..hi].iter().copied().fold(0.0_f32, f32::max);
                let y1 = inner.bottom() - row as f32 * cell_h;
                let y0 = (y1 - cell_h).max(inner.top());
                let cell = Rect::from_min_max(
                    pos2(x0.max(inner.left()), y0),
                    pos2((x0 + col_w + 0.5).min(inner.right()), y1),
                );
                if cell.width() > 0.0 && cell.height() > 0.0 {
                    mesh.add_colored_rect(cell, heat_color(value));
                }
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.line_segment(
            [
                pos2(inner.right() - 0.5, inner.top()),
                pos2(inner.right() - 0.5, inner.bottom()),
            ],
            Stroke::new(1.0_f32, t.accent.gamma_multiply(0.5)),
        );
    }

    fn paint_oscilloscope(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();

        for step in 1..4 {
            let y = inner.top() + inner.height() * step as f32 / 4.0;
            painter.line_segment(
                [pos2(inner.left(), y), pos2(inner.right(), y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(8)),
            );
        }
        for step in 1..8 {
            let x = inner.left() + inner.width() * step as f32 / 8.0;
            painter.line_segment(
                [pos2(x, inner.top()), pos2(x, inner.bottom())],
                Stroke::new(1.0_f32, Color32::from_white_alpha(6)),
            );
        }
        painter.line_segment(
            [
                pos2(inner.left(), inner.center().y),
                pos2(inner.right(), inner.center().y),
            ],
            Stroke::new(1.0_f32, Color32::from_white_alpha(20)),
        );

        let samples = &self.audio_samples;
        if samples.len() < 4 {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "Waiting for audio…",
                theme::mono(10.0),
                Color32::from_white_alpha(110),
            );
            return;
        }

        // Trigger on the first rising zero crossing so the waveform stands still.
        let mut start = 0usize;
        for i in 1..samples.len() / 2 {
            if samples[i - 1] <= 0.0 && samples[i] > 0.0 {
                start = i;
                break;
            }
        }
        let window = (self.sample_rate as usize / 40)
            .min(samples.len() - start)
            .max(2);
        let end = start + window;
        let visible = end - start;
        let color = t.accent;
        let mut points = Vec::with_capacity(visible);
        for (i, &sample) in samples[start..end].iter().enumerate() {
            let x = inner.left() + (i as f32 / (visible - 1) as f32) * inner.width();
            let y = inner.center().y - sample.clamp(-1.0, 1.0) * inner.height() * 0.46;
            points.push(pos2(x, y));
        }
        painter.add(egui::Shape::line(
            points.clone(),
            Stroke::new(3.0_f32, color.gamma_multiply(0.22)),
        ));
        painter.add(egui::Shape::line(points, Stroke::new(1.3_f32, color)));
    }

    fn paint_piano_roll(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();
        let history = &self.note_history;
        if history.is_empty() {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "No notes yet",
                theme::mono(10.0),
                Color32::from_white_alpha(110),
            );
            return;
        }

        const MIN_NOTE: f32 = 24.0;
        const MAX_NOTE: f32 = 96.0;
        let chips = history.len().div_ceil(3).max(1);
        let lane_h = inner.height() / chips as f32;

        for chip in 0..chips {
            let top = inner.top() + chip as f32 * lane_h;
            let bottom = (top + lane_h).min(inner.bottom());
            if chip > 0 {
                painter.line_segment(
                    [pos2(inner.left(), top), pos2(inner.right(), top)],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(14)),
                );
            }
            let mut note = 24;
            while note <= 96 {
                let y = bottom
                    - (note as f32 - MIN_NOTE) / (MAX_NOTE - MIN_NOTE) * (lane_h - 4.0)
                    - 2.0;
                painter.line_segment(
                    [pos2(inner.left(), y), pos2(inner.right(), y)],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(6)),
                );
                note += 12;
            }
            painter.text(
                pos2(inner.left() + 2.0, top + 1.0),
                Align2::LEFT_TOP,
                format!("SID {}", chip + 1),
                theme::mono(7.5),
                Color32::from_white_alpha(70),
            );
        }

        for (voice, samples) in history.iter().enumerate() {
            let chip = voice / 3;
            let top = inner.top() + chip as f32 * lane_h + 2.0;
            let usable = lane_h - 4.0;
            let color = t.voice(voice);
            let n = samples.len();
            if n == 0 {
                continue;
            }
            let mut i = 0;
            while i < n {
                let sample = samples[i];
                if !sample.gate || sample.note == 0 {
                    i += 1;
                    continue;
                }
                let note = sample.note;
                let mut j = i + 1;
                while j < n && samples[j].gate && samples[j].note == note {
                    j += 1;
                }
                let x0 = inner.left() + inner.width() * i as f32 / n as f32;
                let x1 = (inner.left() + inner.width() * j as f32 / n as f32).max(x0 + 1.5);
                let y = top
                    + usable
                    - (note as f32 - MIN_NOTE) / (MAX_NOTE - MIN_NOTE) * usable;
                let h = (usable / (MAX_NOTE - MIN_NOTE) * 1.6).max(1.5);
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, y - h / 2.0), pos2(x1, y + h / 2.0)),
                    CornerRadius::same(1),
                    color,
                );
                i = j;
            }
        }
    }

    fn paint_filter(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();
        let infos = &self.filter_infos;
        if infos.is_empty() {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "No filter state",
                theme::mono(10.0),
                Color32::from_white_alpha(110),
            );
            return;
        }

        let card_rows = self.voice_infos.len().div_ceil(3).max(1);
        let cards_h =
            (card_rows as f32 * 26.0 + (card_rows.saturating_sub(1)) as f32 * 6.0)
                .min(inner.height() * 0.42);
        let curve = Rect::from_min_max(
            inner.min,
            pos2(inner.right(), inner.bottom() - cards_h - 10.0),
        );

        for step in 0..4 {
            let y = curve.top() + curve.height() * step as f32 / 3.0;
            painter.line_segment(
                [pos2(curve.left(), y), pos2(curve.right(), y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(8)),
            );
            painter.text(
                pos2(curve.left() + 2.0, y),
                Align2::LEFT_TOP,
                format!("{} dB", 12 - step * 16),
                theme::mono(7.0),
                Color32::from_white_alpha(70),
            );
        }

        let f_min = 20.0_f32;
        let f_max = 20_000.0_f32;
        for (freq, label) in [(100.0_f32, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
            let frac = (freq / f_min).ln() / (f_max / f_min).ln();
            let x = curve.left() + curve.width() * frac;
            painter.line_segment(
                [pos2(x, curve.top()), pos2(x, curve.bottom())],
                Stroke::new(1.0_f32, Color32::from_white_alpha(7)),
            );
            painter.text(
                pos2(x + 2.0, curve.bottom() - 2.0),
                Align2::LEFT_BOTTOM,
                label,
                theme::mono(7.0),
                Color32::from_white_alpha(80),
            );
        }

        let steps = curve.width().max(2.0) as usize;
        for (index, info) in infos.iter().enumerate() {
            let q = 0.7 + f32::from(info.resonance) * 0.55;
            let cutoff = info.cutoff_hz.clamp(f_min, f_max);
            let mut points = Vec::with_capacity(steps);
            for s in 0..steps {
                let frac = s as f32 / (steps - 1) as f32;
                let freq = f_min * (f_max / f_min).powf(frac);
                let r = freq / cutoff;
                let denom = ((1.0 - r * r).powi(2) + (r / q).powi(2))
                    .sqrt()
                    .max(1e-6);
                let mut magnitude = 0.0;
                if info.lowpass {
                    magnitude += 1.0 / denom;
                }
                if info.bandpass {
                    magnitude += (r / q) / denom;
                }
                if info.highpass {
                    magnitude += r * r / denom;
                }
                let db = 20.0 * magnitude.max(1e-6).log10();
                let y = curve.bottom()
                    - ((db + 36.0) / 48.0).clamp(0.0, 1.0) * curve.height();
                points.push(pos2(curve.left() + frac * curve.width(), y));
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.8_f32, t.voice(index)),
            ));
            let marker = curve.left()
                + (cutoff / f_min).ln() / (f_max / f_min).ln() * curve.width();
            painter.line_segment(
                [pos2(marker, curve.top()), pos2(marker, curve.bottom())],
                Stroke::new(1.0_f32, t.voice(index).gamma_multiply(0.35)),
            );
        }

        let first = &infos[0];
        let mut modes = Vec::new();
        if first.lowpass {
            modes.push("LP");
        }
        if first.bandpass {
            modes.push("BP");
        }
        if first.highpass {
            modes.push("HP");
        }
        let mode_text = if modes.is_empty() {
            "OFF".to_string()
        } else {
            modes.join("+")
        };
        let routed = first
            .routed
            .iter()
            .enumerate()
            .filter_map(|(i, &on)| on.then(|| format!("V{}", i + 1)))
            .collect::<Vec<_>>()
            .join("+");
        let routed = if routed.is_empty() {
            "none".to_string()
        } else {
            routed
        };
        painter.text(
            pos2(curve.left() + 2.0, curve.bottom() - 12.0),
            Align2::LEFT_BOTTOM,
            format!("cutoff {}/2047 · routed {routed}", first.cutoff),
            theme::mono(7.5),
            Color32::from_white_alpha(110),
        );
        painter.text(
            pos2(curve.right() - 2.0, curve.top() + 1.0),
            Align2::RIGHT_TOP,
            format!(
                "{} · Q {:.1} · {:.0} Hz · vol {}",
                mode_text,
                0.7 + f32::from(first.resonance) * 0.55,
                first.cutoff_hz,
                first.volume
            ),
            theme::mono(8.0),
            Color32::from_white_alpha(150),
        );

        let gap = 6.0;
        let card_w = (inner.width() - 2.0 * gap) / 3.0;
        let cards_top = curve.bottom() + 8.0;
        let row_h = cards_h / card_rows as f32;
        for (index, voice) in self.voice_infos.iter().enumerate() {
            let row = index / 3;
            let col = index % 3;
            let x = inner.left() + col as f32 * (card_w + gap);
            let y = cards_top + row as f32 * row_h;
            let card = Rect::from_min_max(pos2(x, y), pos2(x + card_w, y + row_h - 4.0));
            painter.rect_filled(card, CornerRadius::same(4), Color32::from_white_alpha(9));
            let color = t.voice(index);
            painter.rect_filled(
                Rect::from_min_max(card.min, pos2(card.left() + 2.0, card.bottom())),
                CornerRadius::same(2),
                color,
            );
            painter.text(
                pos2(card.left() + 7.0, card.top() + 2.0),
                Align2::LEFT_TOP,
                {
                    let mut flags = String::new();
                    if voice.ring {
                        flags.push('R');
                    }
                    if voice.sync {
                        flags.push('S');
                    }
                    if flags.is_empty() {
                        format!("V{}  {}", index + 1, voice.waveform_name())
                    } else {
                        format!("V{}  {} {flags}", index + 1, voice.waveform_name())
                    }
                },
                theme::mono(8.5),
                color,
            );
            let note = voice.note.map(note_name).unwrap_or_else(|| "-".to_string());
            painter.text(
                pos2(card.left() + 7.0, card.top() + row_h * 0.55),
                Align2::LEFT_TOP,
                format!(
                    "{note} · {:.0} Hz · {:.0}%",
                    voice.freq_hz,
                    voice.pulse_width * 100.0
                ),
                theme::mono(8.0),
                Color32::from_white_alpha(150),
            );
            painter.circle_filled(
                pos2(card.right() - 8.0, card.top() + 7.0),
                2.6,
                if voice.gate {
                    color
                } else {
                    Color32::from_white_alpha(30)
                },
            );
        }
    }

    fn paint_adsr(&self, ui: &egui::Ui, t: &Tokens, rect: Rect) {
        let inner = self.display_chrome(ui, rect);
        let painter = ui.painter();

        let curves = &self.envelope_curves;
        let states = &self.envelope_states;
        if curves.is_empty() {
            painter.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "No envelope data",
                theme::mono(10.0),
                Color32::from_white_alpha(110),
            );
            return;
        }

        let voices = curves.len();
        let row_h = inner.height() / voices as f32;
        for (voice, samples) in curves.iter().enumerate() {
            let base_y = inner.top() + row_h * (voice as f32 + 0.5);
            let color = t.voice(voice);
            painter.line_segment(
                [pos2(inner.left(), base_y), pos2(inner.right(), base_y)],
                Stroke::new(1.0_f32, color.gamma_multiply(0.14)),
            );
            painter.text(
                pos2(inner.left() + 2.0, base_y),
                Align2::LEFT_CENTER,
                format!("V{}", voice + 1),
                theme::mono(8.0),
                color.gamma_multiply(0.75),
            );

            let state_history = states.get(voice);
            let count = samples.len();
            let mut i = 0;
            while i < count {
                let stage = state_history
                    .and_then(|history| history.get(i))
                    .copied()
                    .unwrap_or(1);
                let mut j = i + 1;
                while j < count {
                    let next = state_history
                        .and_then(|history| history.get(j))
                        .copied()
                        .unwrap_or(1);
                    if next != stage {
                        break;
                    }
                    j += 1;
                }
                let mut points = Vec::with_capacity(j - i + 1);
                for k in i..j {
                    let x = inner.left()
                        + inner.width() * k as f32 / (count.saturating_sub(1)).max(1) as f32;
                    let y = base_y - (samples[k].clamp(0.0, 1.0) - 0.5) * (row_h * 0.78);
                    points.push(pos2(x, y));
                }
                if points.len() >= 2 {
                    painter.add(egui::Shape::line(
                        points,
                        Stroke::new(1.3_f32, stage_color(stage)),
                    ));
                } else if let Some(point) = points.first() {
                    painter.circle_filled(*point, 1.2, stage_color(stage));
                }
                i = j;
            }
        }

        // Legend last, so the traces cannot paint over it. Each label is
        // measured: "D/S" is wider than "A" and "R", and a fixed step would
        // leave an uneven gap before it.
        let legend_gap = 6.0;
        let mut legend_x = inner.right() - 2.0;
        for (label, color) in [
            ("R", stage_color(2)),
            ("D/S", stage_color(1)),
            ("A", stage_color(0)),
        ] {
            let galley = painter.layout_no_wrap(label.to_owned(), theme::mono(8.5), color);
            let width = galley.size().x;
            painter.galley(pos2(legend_x - width, inner.top() + 1.0), galley, color);
            legend_x -= width + legend_gap;
        }
    }

    fn playlist_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::Frame::NONE
            .inner_margin(Margin {
                left: 28,
                right: 28,
                top: 24,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Playlist")
                        .font(theme::semibold(22.0))
                        .color(t.text),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Library")
                            .font(theme::medium(11.5))
                            .color(t.text_muted),
                    );

                    let current = playlist_display_name(&self.playlist_path);
                    let files = self.playlist_files.clone();
                    let mut chosen: Option<std::path::PathBuf> = None;
                    egui::ComboBox::from_id_salt("playlist_library")
                        .selected_text(
                            RichText::new(current).font(theme::regular(12.0)),
                        )
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            if files.is_empty() {
                                ui.label(
                                    RichText::new("No playlists yet")
                                        .font(theme::regular(11.5))
                                        .color(t.text_muted),
                                );
                            }
                            for path in &files {
                                let selected = *path == self.playlist_path;
                                if ui
                                    .selectable_label(
                                        selected,
                                        playlist_display_name(path),
                                    )
                                    .clicked()
                                {
                                    chosen = Some(path.clone());
                                }
                            }
                        });
                    if let Some(path) = chosen {
                        // Picking from the library just makes the list active;
                        // it must not start playback on its own.
                        self.select_playlist(path);
                    }
                    if secondary_button(ui, &t, "New…").clicked() {
                        self.show_new_playlist = true;
                        self.new_playlist_name.clear();
                        self.new_playlist_error = None;
                    }
                    if secondary_button(ui, &t, "Open…").clicked() {
                        self.open_file_dialog();
                    }
                    if secondary_button(ui, &t, "Rename").clicked() {
                        self.rename_playlist_name = playlist_display_name(&self.playlist_path);
                        self.rename_playlist_error = None;
                        self.show_rename_playlist = true;
                    }
                    if secondary_button(ui, &t, "Delete").clicked() {
                        self.confirm_action = Some(ConfirmAction::DeletePlaylist);
                    }
                    if secondary_button(ui, &t, "Save As").clicked() {
                        self.save_playlist_as();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, &t, "folder", 26.0, false, "Open playlist folder").clicked() {
                            self.open_playlist_folder();
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                files.len(),
                                if files.len() == 1 {
                                    "playlist"
                                } else {
                                    "playlists"
                                }
                            ))
                            .font(theme::regular(11.5))
                            .color(t.text_faint),
                        );
                    });
                });

                let summary = format!(
                    "{} — {} {}",
                    playlist_display_name(&self.playlist_path),
                    self.playlist.len(),
                    if self.playlist.len() == 1 {
                        "track"
                    } else {
                        "tracks"
                    }
                );
                ui.label(
                    RichText::new(summary)
                        .font(theme::regular(11.5))
                        .color(t.text_muted),
                );

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, &t, "Add files").clicked() {
                        self.add_files_dialog();
                    }
                    if secondary_button(ui, &t, "Clear").clicked() {
                        self.confirm_action = Some(ConfirmAction::ClearTracks);
                    }
                    if secondary_button(ui, &t, "Remove").clicked() {
                        self.confirm_action = Some(ConfirmAction::RemoveTrack);
                    }
                    if secondary_button(ui, &t, "Play all").clicked() {
                        self.play_all();
                        self.view = View::Player;
                        self.last_view = View::Player;
                    }
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                let mut play = None;
                let mut select = None;
                let rows: Vec<(usize, String, String, Option<u16>)> = self
                    .playlist
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (i, e.display_name.clone(), e.source.clone(), e.subsong))
                    .collect();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new("This playlist is empty. Use Add files, New…, or the HVSC browser.")
                                    .font(theme::regular(13.0))
                                    .color(t.text_muted),
                            );
                        }
                        for (idx, name, source, subsong) in rows {
                            let selected = idx == self.selected_playlist_idx;
                            let row = playlist_row(ui, &t, idx, &name, &source, subsong, selected);
                            if row.clicked() {
                                select = Some(idx);
                            }
                            if row.double_clicked() {
                                play = Some(idx);
                            }
                        }
                    });
                if let Some(idx) = select {
                    self.selected_playlist_idx = idx;
                }
                if let Some(idx) = play {
                    self.play_playlist_index(idx);
                    self.view = View::Player;
                    self.last_view = View::Player;
                }
            });
    }

    fn hvsc_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::Frame::NONE
            .inner_margin(Margin {
                left: 28,
                right: 28,
                top: 24,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("High Voltage SID Collection")
                        .font(theme::semibold(22.0))
                        .color(t.text),
                );
                ui.label(
                    RichText::new("Browse the mirror or search titles, authors and paths.")
                        .font(theme::regular(12.0))
                        .color(t.text_muted),
                );
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    let mut search = self.hvsc_search_query.clone();
                    let resp = search_box(ui, &t, &mut search, 360.0, "Search titles, authors or paths");
                    if search != self.hvsc_search_query {
                        self.hvsc_search_query = search;
                    }
                    let submitted = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if primary_button(ui, &t, "Search").clicked() || submitted {
                        self.start_hvsc_search();
                    }
                    if !self.hvsc_search_results.is_empty() && secondary_button(ui, &t, "Browse").clicked() {
                        self.hvsc_search_results.clear();
                    }
                });

                ui.add_space(8.0);
                if !self.hvsc_search_results.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} results · double-click to play", self.hvsc_search_results.len()))
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                        );
                    });
                    ui.add_space(4.0);
                    let mut play_path = None;
                    let results = self.hvsc_search_results.clone();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (i, path) in results.iter().enumerate() {
                                let selected = i == self.hvsc_search_selected;
                                let row = result_row(ui, &t, "music", path, "", selected);
                                if row.clicked() {
                                    self.hvsc_search_selected = i;
                                }
                                if row.double_clicked() {
                                    play_path = Some(path.clone());
                                }
                            }
                        });
                    if let Some(path) = play_path {
                        self.hvsc_fetch(&path);
                    }
                    return;
                }

                if self.hvsc_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Contacting the HVSC mirror…")
                                .font(theme::regular(12.0))
                                .color(t.text_muted),
                        );
                    });
                    return;
                }

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&self.hvsc_browser.current_path)
                            .font(theme::mono(11.5))
                            .color(t.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.hvsc_browser.can_go_up() && secondary_button(ui, &t, "Up").clicked() {
                            self.hvsc_go_up();
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();

                let mut navigate = None;
                let mut fetch = None;
                let entries = self.hvsc_browser.entries.clone();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in &entries {
                            let icon = if entry.is_dir { "folder" } else { "music" };
                            let subtitle = if entry.is_dir { "Directory" } else { "SID tune" };
                            let row = result_row(ui, &t, icon, &entry.name, subtitle, false);
                            if row.clicked() {
                                if entry.is_dir {
                                    navigate = Some(entry.path.clone());
                                } else {
                                    fetch = Some(entry.path.clone());
                                }
                            }
                        }
                    });
                if let Some(path) = navigate {
                    self.hvsc_open_dir(&path);
                }
                if let Some(path) = fetch {
                    self.hvsc_fetch(&path);
                }
            });
    }

    fn settings_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(Margin {
                        left: 32,
                        right: 32,
                        top: 26,
                        bottom: 26,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Settings")
                                .font(theme::semibold(22.0))
                                .color(t.text),
                        );
                        ui.label(
                            RichText::new("Appearance, playback and emulation preferences.")
                                .font(theme::regular(12.0))
                                .color(t.text_muted),
                        );
                        ui.add_space(16.0);

                        settings_card(ui, &t, "Appearance", |ui| {
                            ui.label(
                                RichText::new("Theme")
                                    .font(theme::medium(12.5))
                                    .color(t.text),
                            );
                            let labels = [
                                ThemePreference::System.label(),
                                ThemePreference::Light.label(),
                                ThemePreference::Dark.label(),
                            ];
                            let current = match self.theme_pref {
                                ThemePreference::System => 0,
                                ThemePreference::Light => 1,
                                ThemePreference::Dark => 2,
                            };
                            if let Some(choice) = segmented(ui, &t, current, &labels) {
                                self.theme_pref = match choice {
                                    1 => ThemePreference::Light,
                                    2 => ThemePreference::Dark,
                                    _ => ThemePreference::System,
                                };
                            }
                            ui.label(
                                RichText::new("System follows the operating system's light/dark setting.")
                                    .font(theme::regular(11.5))
                                    .color(t.text_faint),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Visualiser")
                                    .font(theme::medium(12.5))
                                    .color(t.text),
                            );
                            let mut mode = self.viz_mode;
                            egui::ComboBox::from_id_salt("settings-viz")
                                .width(180.0)
                                .selected_text(mode.label())
                                .show_ui(ui, |ui| {
                                    for option in VizMode::ALL {
                                        ui.selectable_value(&mut mode, option, option.label());
                                    }
                                });
                            if mode != self.viz_mode {
                                self.viz_mode = mode;
                            }
                        });

                        settings_card(ui, &t, "Playback", |ui| {
                            property_row(ui, &t, "SID chip", self.chip_label());
                            property_row(ui, &t, "Clock", self.clock_label());
                            property_row(ui, &t, "Chips in tune", self.sid_count_label());
                            property_row(ui, &t, "Subsong", &format!("{} / {}", self.current_song, self.total_songs));
                            ui.add_space(8.0);
                            let mut ekv = self.ekv_filter;
                            if ui.checkbox(&mut ekv, "EKV transistor filter").changed() {
                                self.set_ekv_filter(ekv);
                            }
                            if secondary_button(ui, &t, "Cycle SID chip model").clicked() {
                                self.cycle_chip_model();
                            }
                        });

                        settings_card(ui, &t, "Audio", |ui| {
                            property_row(ui, &t, "Output rate", &format!("{} Hz", self.sample_rate));
                            property_row(ui, &t, "Resampling", &format!("{:?}", self.sampling_method));
                            property_row(ui, &t, "Voices", &self.vu_meter.voice_count().to_string());
                            property_row(ui, &t, "Status", if self.paused { "Paused" } else { "Playing" });
                        });
                    });
            });
    }

    fn new_playlist_window(&mut self, ctx: &egui::Context) {
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

    fn confirm_window(&mut self, ctx: &egui::Context) {
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

    fn rename_playlist_window(&mut self, ctx: &egui::Context) {
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

    fn about_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::Window::new("About SidCraft")
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
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    // The application artwork from `icon/sidcraft.png`.
                    let (icon_rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                    match crate::app_icon::texture(ctx) {
                        Some(texture) => {
                            egui::Image::from_texture(&texture)
                                .fit_to_exact_size(icon_rect.size())
                                .paint_at(ui, icon_rect);
                        }
                        None => {
                            // Fall back to the vector mark if the PNG cannot be
                            // decoded or uploaded.
                            ui.painter()
                                .rect_filled(icon_rect, CornerRadius::same(10), t.accent_soft);
                            paint_icon(ui.painter(), icon_rect.shrink(9.0), "music", t.accent, 1.8);
                        }
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("SidCraft")
                                .font(theme::semibold(19.0))
                                .color(t.text),
                        );
                        ui.label(
                            RichText::new("SID player and HVSC browser")
                                .font(theme::regular(12.0))
                                .color(t.text_muted),
                        );
                    });
                });
                ui.add_space(14.0);
                property_row_right(ui, &t, "Version", env!("CARGO_PKG_VERSION"));
                property_row_right(ui, &t, "Coded by", "emarti, Murat Özdemir");
                property_row_right(ui, &t, "Engine", "residfp + mos6502");
                property_row_right(ui, &t, "Interface", "egui / eframe");
                ui.add_space(14.0);
                // `Align::Min` on the cross axis keeps the button row hugging
                // its content; `Align::Center` would stretch the row over the
                // whole remaining height and leave a large gap.
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if primary_button(ui, &t, "Close").clicked() {
                        self.show_about = false;
                    }
                });
            });
    }

    // ── small presentation helpers ──────────────────────────────────────────

    fn track_title(&self) -> String {
        if self.sid_file.name.is_empty() {
            "No tune loaded".to_string()
        } else {
            self.sid_file.name.clone()
        }
    }

    fn chip_label(&self) -> &'static str {
        match self.chip_models.first() {
            Some(ChipModel::Mos8580) => "MOS 8580",
            _ => "MOS 6581",
        }
    }

    fn clock_label(&self) -> &'static str {
        if self.sid_file.flags & 0x08 != 0 {
            "NTSC"
        } else {
            "PAL"
        }
    }

    fn sid_count_label(&self) -> &'static str {
        match self.chip_models.len() {
            2 => "2SID",
            3 => "3SID",
            _ => "1SID",
        }
    }
}

// ── reusable widgets ────────────────────────────────────────────────────────

fn document_tab(ui: &mut egui::Ui, t: &Tokens, title: &str, selected: bool) -> Response {
    let font = theme::medium(12.5);
    let text_w = text_width(ui, title, &font);
    let width = (text_w + 52.0).clamp(96.0, 240.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let fill = if selected {
        t.card
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect.shrink(1.0),
        CornerRadius::same(7),
        fill,
        if selected {
            Stroke::new(1.0_f32, t.border)
        } else {
            Stroke::NONE
        },
        StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        Rect::from_min_size(rect.left_center() + vec2(9.0, -8.0), vec2(16.0, 16.0)),
        "music",
        if selected { t.accent } else { t.icon },
        1.4,
    );
    paint_text_elided(
        ui,
        rect.left_center() + vec2(31.0, 0.0),
        Align2::LEFT_CENTER,
        title,
        font,
        if selected { t.text } else { t.text_muted },
        rect.width() - 42.0,
    );
    if selected {
        ui.painter().line_segment(
            [
                pos2(rect.left() + 10.0, rect.bottom() - 2.0),
                pos2(rect.right() - 10.0, rect.bottom() - 2.0),
            ],
            Stroke::new(2.0_f32, t.accent),
        );
    }
    theme::hand(resp).on_hover_text(title)
}

fn mode_tab(ui: &mut egui::Ui, t: &Tokens, label: &str, selected: bool) -> Response {
    let font = theme::medium(13.0);
    let width = text_width(ui, label, &font) + 22.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink(2.0), CornerRadius::same(6), t.hover);
    }
    ui.painter().text(
        rect.center() + vec2(0.0, -1.0),
        Align2::CENTER_CENTER,
        label,
        font,
        if selected { t.text } else { t.text_muted },
    );
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(rect.left() + 8.0, rect.bottom() - 2.0), vec2(rect.width() - 16.0, 2.0)),
            CornerRadius::same(1),
            t.accent,
        );
    }
    theme::hand(resp)
}

fn icon_button(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    size: f32,
    selected: bool,
    tip: &str,
) -> Response {
    // The glyph keeps the classic inset so every plain icon button looks the
    // same; only the explicit variant below changes the drawn size.
    icon_button_glyph(
        ui,
        t,
        icon,
        size,
        size - size * 0.54,
        (size * 0.07).clamp(1.2, 1.9),
        selected,
        tip,
    )
}

/// Like [`icon_button`] but with an explicit glyph size and stroke width, so a
/// compact hit area can still draw an icon as large as the transport arrows.
fn icon_button_glyph(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    size: f32,
    glyph: f32,
    stroke: f32,
    selected: bool,
    tip: &str,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let down = resp.is_pointer_button_down_on();
    if selected || resp.hovered() || down {
        let fill = if selected {
            t.accent_soft
        } else if down {
            t.pressed
        } else {
            t.hover
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    }
    let color = if selected { t.accent } else { t.text };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(glyph)),
        icon,
        color,
        stroke,
    );
    theme::hand(resp).on_hover_text(tip)
}

fn transport_button(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    size: f32,
    primary: bool,
    tip: &str,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if primary {
        let fill = if resp.hovered() {
            t.accent.gamma_multiply(1.1)
        } else {
            t.accent
        };
        ui.painter().circle_filled(rect.center(), size * 0.5, fill);
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size * 0.44));
        paint_icon(ui.painter(), icon_rect, icon, Color32::WHITE, 1.8);
    } else {
        if resp.hovered() || resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), t.hover);
        }
        paint_icon(
            ui.painter(),
            rect.shrink(size * 0.27),
            icon,
            t.text,
            1.6,
        );
    }
    theme::hand(resp).on_hover_text(tip)
}

fn toolbar_button(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    label: &str,
    selected: bool,
) -> Response {
    let font = theme::medium(12.5);
    let width = text_width(ui, label, &font) + 42.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    let fill = if selected {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    paint_icon(
        ui.painter(),
        Rect::from_min_size(rect.left_center() + vec2(8.0, -7.0), vec2(14.0, 14.0)),
        icon,
        if selected { t.accent_text } else { t.icon },
        1.4,
    );
    paint_text_elided(
        ui,
        rect.left_center() + vec2(27.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        font,
        if selected { t.accent_text } else { t.text },
        rect.width() - 34.0,
    );
    theme::hand(resp)
}

fn search_box(
    ui: &mut egui::Ui,
    t: &Tokens,
    text: &mut String,
    width: f32,
    hint: &str,
) -> Response {
    egui::Frame::NONE
        .fill(t.field)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        ui.cursor().left_center() + vec2(8.0, 0.0),
                        vec2(14.0, 14.0),
                    ),
                    "search",
                    t.text_faint,
                    1.4,
                );
                ui.add_space(18.0);

                let has_text = !text.is_empty();
                let edit_width = if has_text {
                    (width - 62.0).max(40.0)
                } else {
                    (width - 40.0).max(60.0)
                };
                let edit = ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(RichText::new(hint).color(t.text_faint))
                        .font(theme::regular(12.5))
                        .text_color(t.text)
                        .frame(false)
                        .desired_width(edit_width),
                );

                if has_text {
                    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    let resp = resp.on_hover_text("Clear");
                    let bg = if resp.hovered() || resp.clicked() {
                        t.hover
                    } else {
                        t.border
                    };
                    let icon_color = if resp.hovered() {
                        t.text
                    } else {
                        t.text_muted
                    };
                    ui.painter().circle_filled(rect.center(), 8.0, bg);
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(rect.center(), vec2(10.0, 10.0)),
                        "x",
                        icon_color,
                        1.6,
                    );
                    if resp.clicked() {
                        text.clear();
                    }
                }

                edit
            })
            .inner
        })
        .inner
}

fn tool_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    label: &str,
    subtitle: &str,
    selected: bool,
) -> Response {
    let height = if subtitle.is_empty() { 36.0 } else { 46.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let fill = if selected {
        t.accent_soft
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    paint_icon(
        ui.painter(),
        Rect::from_min_size(rect.left_center() + vec2(8.0, -(height * 0.22 + 9.0)), vec2(18.0, 18.0)),
        icon,
        if selected { t.accent } else { t.icon },
        1.5,
    );
    let text_left = rect.left_center() + vec2(36.0, if subtitle.is_empty() { 0.0 } else { -8.0 });
    paint_text_elided(
        ui,
        text_left,
        Align2::LEFT_CENTER,
        label,
        theme::medium(13.0),
        if selected { t.accent_text } else { t.text },
        rect.width() - 46.0,
    );
    if !subtitle.is_empty() {
        paint_text_elided(
            ui,
            rect.left_center() + vec2(36.0, 10.0),
            Align2::LEFT_CENTER,
            subtitle,
            theme::regular(10.5),
            t.text_faint,
            rect.width() - 46.0,
        );
    }
    theme::hand(resp)
}

fn playlist_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    idx: usize,
    name: &str,
    source: &str,
    subsong: Option<u16>,
    selected: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
    let fill = if selected {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    paint_icon(
        ui.painter(),
        Rect::from_min_size(rect.left_center() + vec2(10.0, -8.0), vec2(16.0, 16.0)),
        "music",
        if selected { t.accent } else { t.icon },
        1.4,
    );
    let right = if let Some(sub) = subsong {
        format!("subsong {sub}")
    } else {
        String::new()
    };
    let name_width = (rect.width() - 72.0).max(40.0);
    paint_text_elided(
        ui,
        rect.left_center() + vec2(34.0, -8.0),
        Align2::LEFT_CENTER,
        &format!("{:02}. {name}", idx + 1),
        theme::medium(12.5),
        if selected { t.text } else { t.text },
        name_width,
    );
    paint_text_elided(
        ui,
        rect.left_center() + vec2(34.0, 10.0),
        Align2::LEFT_CENTER,
        source,
        theme::regular(10.0),
        t.text_faint,
        name_width,
    );
    if !right.is_empty() {
        ui.painter().text(
            rect.right_center() - vec2(10.0, 0.0),
            Align2::RIGHT_CENTER,
            right,
            theme::regular(10.0),
            t.text_faint,
        );
    }
    theme::hand(resp)
}

fn result_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    title: &str,
    subtitle: &str,
    selected: bool,
) -> Response {
    let height = if subtitle.is_empty() { 38.0 } else { 48.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let fill = if selected {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    paint_icon(
        ui.painter(),
        Rect::from_min_size(rect.left_center() + vec2(10.0, -9.0), vec2(18.0, 18.0)),
        icon,
        if icon == "folder" { t.accent } else { t.icon },
        1.5,
    );
    paint_text_elided(
        ui,
        rect.left_center() + vec2(38.0, if subtitle.is_empty() { 0.0 } else { -8.0 }),
        Align2::LEFT_CENTER,
        title,
        theme::regular(12.5),
        t.text,
        rect.width() - 52.0,
    );
    if !subtitle.is_empty() {
        paint_text_elided(
            ui,
            rect.left_center() + vec2(38.0, 10.0),
            Align2::LEFT_CENTER,
            subtitle,
            theme::regular(10.5),
            t.text_faint,
            rect.width() - 52.0,
        );
    }
    theme::hand(resp)
}

fn tool_card(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    title: &str,
    blurb: &str,
    accent: Color32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 104.0), Sense::click());
    let fill = if resp.hovered() { t.hover } else { t.card };
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        fill,
        Stroke::new(1.0_f32, t.divider),
        StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        Rect::from_min_size(rect.min + vec2(14.0, 14.0), vec2(22.0, 22.0)),
        icon,
        accent,
        1.7,
    );
    paint_text_elided(
        ui,
        rect.min + vec2(44.0, 24.0),
        Align2::LEFT_CENTER,
        title,
        theme::semibold(13.5),
        t.text,
        rect.width() - 56.0,
    );
    let mut job = egui::text::LayoutJob::simple(
        blurb.to_owned(),
        theme::regular(11.0),
        t.text_muted,
        rect.width() - 28.0,
    );
    job.wrap.max_rows = 2;
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter()
        .galley(rect.min + vec2(14.0, 44.0), galley, t.text_muted);
    ui.painter().text(
        rect.left_bottom() + vec2(14.0, -14.0),
        Align2::LEFT_CENTER,
        "Open",
        theme::medium(11.5),
        t.accent_text,
    );
    theme::hand(resp)
}

fn primary_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> Response {
    let font = theme::medium(12.5);
    let width = text_width(ui, label, &font) + 26.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let fill = if resp.hovered() {
        t.accent.gamma_multiply(1.08)
    } else {
        t.accent
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        Color32::WHITE,
    );
    theme::hand(resp)
}

fn secondary_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> Response {
    let font = theme::medium(12.5);
    let width = text_width(ui, label, &font) + 24.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let fill = if resp.hovered() { t.hover } else { t.card };
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        fill,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        t.text,
    );
    theme::hand(resp)
}

fn ghost_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> Response {
    let font = theme::medium(12.5);
    let width = text_width(ui, label, &font) + 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), t.hover);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        t.accent_text,
    );
    theme::hand(resp)
}

fn segmented(
    ui: &mut egui::Ui,
    t: &Tokens,
    current: usize,
    labels: &[&str],
) -> Option<usize> {
    let mut chosen = None;
    egui::Frame::NONE
        .fill(t.field)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, label) in labels.iter().enumerate() {
                    let font = theme::medium(12.0);
                    let width = text_width(ui, label, &font) + 20.0;
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
                    let selected = i == current;
                    let fill = if selected {
                        t.accent_soft
                    } else if resp.hovered() {
                        t.hover
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        *label,
                        font,
                        if selected { t.accent_text } else { t.text_muted },
                    );
                    if theme::hand(resp).clicked() {
                        chosen = Some(i);
                    }
                }
            });
        });
    chosen
}

fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, color: Color32) {
    let font = theme::medium(10.0);
    let width = text_width(ui, text, &font) + 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

fn property_row(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [92.0, 18.0],
            egui::Label::new(
                RichText::new(label)
                    .font(theme::regular(11.5))
                    .color(t.text_faint),
            )
            .truncate(),
        );
        ui.label(
            RichText::new(value)
                .font(theme::regular(11.5))
                .color(t.text_muted),
        );
    });
}

fn property_row_right(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &str) {
    ui.horizontal(|ui| {
        // Fixed-width cell with a right-to-left layout so the label hugs the
        // value column instead of the left edge.
        let _ = ui.allocate_ui_with_layout(
            vec2(92.0, 18.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(label)
                            .font(theme::regular(11.5))
                            .color(t.text_faint),
                    )
                    .truncate(),
                );
            },
        );
        ui.label(
            RichText::new(value)
                .font(theme::regular(11.5))
                .color(t.text_muted),
        );
    });
}

fn spectrum_color(t: &Tokens, value: f32) -> Color32 {
    let hot = Color32::from_rgb(0xFF, 0xC4, 0x5C);
    mix_color(t.accent, hot, value.clamp(0.0, 1.0).powf(1.6))
}

fn mix_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |x: u8, y: u8| {
        #[allow(clippy::cast_possible_truncation)]
        let value = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
        value as u8
    };
    Color32::from_rgb(
        blend(a.r(), b.r()),
        blend(a.g(), b.g()),
        blend(a.b(), b.b()),
    )
}

fn heat_color(value: f32) -> Color32 {
    let v = value.clamp(0.0, 1.0);
    let (r, g, b) = if v < 0.5 {
        let k = v / 0.5;
        (
            lerp(0.04, 0.16, k),
            lerp(0.07, 0.50, k),
            lerp(0.14, 0.85, k),
        )
    } else {
        let k = (v - 0.5) / 0.5;
        (
            lerp(0.16, 0.98, k),
            lerp(0.50, 0.85, k),
            lerp(0.85, 0.30, k),
        )
    };
    #[allow(clippy::cast_possible_truncation)]
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn stage_color(stage: u8) -> Color32 {
    match stage {
        0 => Color32::from_rgb(0x6B, 0xD6, 0x7C),
        2 => Color32::from_rgb(0xF2, 0x6D, 0xA6),
        _ => Color32::from_rgb(0xF2, 0xB3, 0x3C),
    }
}

fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(note) / 12 - 1;
    format!("{}{}", NAMES[usize::from(note % 12)], octave)
}

fn section_title(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    ui.label(
        RichText::new(label.to_uppercase())
            .font(theme::semibold(10.5))
            .color(t.text_faint),
    );
}

fn voice_meters(ui: &mut egui::Ui, t: &Tokens, levels: &[f32], peaks: &[f32]) {
    for (i, level) in levels.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("V{}", i + 1))
                    .font(theme::mono(9.5))
                    .color(t.voice(i)),
            );
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
            let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
            ui.painter().rect_filled(track, CornerRadius::same(2), t.pressed);
            let fill = Rect::from_min_size(
                track.min,
                vec2(track.width() * level.clamp(0.0, 1.0), track.height()),
            );
            ui.painter().rect_filled(fill, CornerRadius::same(2), t.voice(i));
            let peak = peaks.get(i).copied().unwrap_or(0.0);
            let x = track.left() + track.width() * peak.clamp(0.0, 1.0);
            ui.painter().line_segment(
                [pos2(x, track.top() - 2.0), pos2(x, track.bottom() + 2.0)],
                Stroke::new(1.5_f32, t.text),
            );
        });
    }
}

fn seek_bar(ui: &mut egui::Ui, t: &Tokens, fraction: f32) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
    let hovered = resp.hovered() || resp.dragged();
    let preview_id = resp.id.with("seek-preview");
    let pointer_fraction = ui.ctx().pointer_latest_pos().map(|pos| {
        ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0)
    });

    // While dragging, follow the pointer without seeking; committing on release
    // avoids restarting the emulator on every frame of the drag.
    let mut shown = fraction.clamp(0.0, 1.0);
    if resp.dragged() {
        if let Some(f) = pointer_fraction {
            shown = f;
            ui.data_mut(|d| d.insert_temp(preview_id, f));
        }
    } else if resp.drag_stopped() {
        if let Some(f) = ui.data(|d| d.get_temp::<f32>(preview_id)) {
            shown = f;
        } else if let Some(f) = pointer_fraction {
            shown = f;
        }
    }

    ui.painter()
        .rect_filled(track, CornerRadius::same(2), if hovered { t.pressed } else { t.hover });
    let fill_w = track.width() * shown;
    ui.painter().rect_filled(
        Rect::from_min_size(track.min, vec2(fill_w, track.height())),
        CornerRadius::same(2),
        t.accent,
    );
    let knob = pos2(track.left() + fill_w, track.center().y);
    ui.painter().circle_filled(knob, if hovered { 6.0 } else { 5.0 }, t.accent);
    ui.painter()
        .circle_stroke(knob, if hovered { 6.0 } else { 5.0 }, Stroke::new(1.5_f32, t.card));

    if resp.clicked() {
        return pointer_fraction;
    }
    if resp.drag_stopped() {
        return Some(shown);
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    None
}

fn volume_bar(ui: &mut egui::Ui, t: &Tokens, fraction: f32, width: f32) -> Option<f32> {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(width, 20.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
    let hovered = resp.hovered() || resp.dragged();
    let pointer_fraction = ui.ctx().pointer_latest_pos().map(|pos| {
        ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0)
    });

    // While dragging, follow the pointer directly so the knob moves live and
    // the caller can apply the new gain every frame.
    let shown = if resp.dragged() {
        pointer_fraction.unwrap_or_else(|| fraction.clamp(0.0, 1.0))
    } else {
        fraction.clamp(0.0, 1.0)
    };

    ui.painter()
        .rect_filled(track, CornerRadius::same(2), if hovered { t.pressed } else { t.hover });
    let fill_w = track.width() * shown;
    ui.painter().rect_filled(
        Rect::from_min_size(track.min, vec2(fill_w, track.height())),
        CornerRadius::same(2),
        t.accent,
    );
    let knob = pos2(track.left() + fill_w, track.center().y);
    ui.painter().circle_filled(knob, if hovered { 6.0 } else { 5.0 }, t.accent);
    ui.painter()
        .circle_stroke(knob, if hovered { 6.0 } else { 5.0 }, Stroke::new(1.5_f32, t.card));

    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    if resp.dragged() || resp.clicked() {
        pointer_fraction
    } else if resp.drag_stopped() {
        pointer_fraction.or(Some(shown))
    } else {
        None
    }
}

fn settings_card(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .font(theme::semibold(15.0))
                    .color(t.text),
            );
            ui.add_space(8.0);
            body(ui);
        });
    ui.add_space(14.0);
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x)
}

fn paint_text_elided(
    ui: &egui::Ui,
    anchor: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: max_width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        ..Default::default()
    };
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = align.anchor_size(anchor, galley.size()).min;
    ui.painter().galley(pos, galley, color);
}

// ── vector icons ────────────────────────────────────────────────────────────
//
// A small, original set of stroke icons in the Lucide spirit: 24-unit-grid
// geometry scaled into the allocation, so the shell does not depend on an icon
// font or an external asset file.

/// Draws the light/dark toggle: a sun while the dark theme is active and a
/// crescent while the light theme is active. `bg` is the colour behind the
/// icon, used to cut the crescent out of a filled disc.
fn paint_theme_icon(
    painter: &egui::Painter,
    rect: Rect,
    dark: bool,
    color: Color32,
    bg: Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s * 0.1).clamp(1.4, 2.2), color);

    if dark {
        // Sun: a filled core with eight rays.
        painter.circle_filled(c, s * 0.19, color);
        for i in 0..8 {
            let a = std::f32::consts::TAU * i as f32 / 8.0;
            painter.line_segment(
                [
                    pos2(c.x + a.cos() * s * 0.31, c.y + a.sin() * s * 0.31),
                    pos2(c.x + a.cos() * s * 0.46, c.y + a.sin() * s * 0.46),
                ],
                stroke,
            );
        }
    } else {
        // Moon: a disc with an offset disc erased, leaving a clean crescent.
        painter.circle_filled(c, s * 0.42, color);
        painter.circle_filled(
            pos2(c.x + s * 0.2, c.y - s * 0.17),
            s * 0.36,
            bg,
        );
    }
}

fn paint_icon(painter: &egui::Painter, rect: Rect, name: &str, color: Color32, width: f32) {
    let r = rect.shrink(rect.width().min(rect.height()) * 0.06);
    let c = r.center();
    let s = r.width().min(r.height());
    let stroke = Stroke::new(width.max(1.0), color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], stroke);
    let poly = |points: Vec<Pos2>| {
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    };

    match name {
        "home" => {
            let body = Rect::from_min_max(pos2(r.left(), c.y), pos2(r.right(), r.bottom()));
            painter.rect_stroke(body, CornerRadius::same(1), stroke, StrokeKind::Inside);
            line(pos2(r.left() - s * 0.02, c.y), pos2(c.x, r.top()));
            line(pos2(c.x, r.top()), pos2(r.right() + s * 0.02, c.y));
            line(
                pos2(c.x - s * 0.12, r.bottom()),
                pos2(c.x - s * 0.12, c.y + s * 0.2),
            );
            line(
                pos2(c.x + s * 0.12, r.bottom()),
                pos2(c.x + s * 0.12, c.y + s * 0.2),
            );
            line(
                pos2(c.x - s * 0.12, c.y + s * 0.2),
                pos2(c.x + s * 0.12, c.y + s * 0.2),
            );
        }
        "music" => {
            let head = pos2(r.left() + s * 0.24, r.bottom() - s * 0.22);
            painter.circle_filled(head, s * 0.12, color);
            line(
                pos2(head.x + s * 0.12, head.y),
                pos2(head.x + s * 0.12, r.top() + s * 0.08),
            );
            line(
                pos2(head.x + s * 0.12, r.top() + s * 0.08),
                pos2(r.right() - s * 0.08, r.top() + s * 0.18),
            );
            line(
                pos2(r.right() - s * 0.08, r.top() + s * 0.18),
                pos2(r.right() - s * 0.08, r.top() + s * 0.36),
            );
        }
        "shuffle" => {
            // Two curved arrows (shuffle gesture).
            let quad = |p0: Pos2, c: Pos2, p1: Pos2, steps: usize, out: &mut Vec<Pos2>| {
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let u = 1.0 - t;
                    let x = u * u * p0.x + 2.0 * u * t * c.x + t * t * p1.x;
                    let y = u * u * p0.y + 2.0 * u * t * c.y + t * t * p1.y;
                    out.push(pos2(x, y));
                }
            };
            let mut a1 = Vec::new();
            quad(
                pos2(r.left() + s * 0.2, c.y + s * 0.28),
                pos2(r.left() + s * 0.5, c.y - s * 0.42),
                pos2(r.right() - s * 0.2, c.y + s * 0.28),
                24,
                &mut a1,
            );
            painter.add(egui::Shape::line(a1, stroke));
            let mut a2 = Vec::new();
            quad(
                pos2(r.right() - s * 0.2, c.y - s * 0.28),
                pos2(r.right() - s * 0.5, c.y + s * 0.42),
                pos2(r.left() + s * 0.2, c.y - s * 0.28),
                24,
                &mut a2,
            );
            painter.add(egui::Shape::line(a2, stroke));
        }
        "repeat" => {
            // Two curved arrows in the same rotational direction (repeat).
            let quad = |p0: Pos2, c: Pos2, p1: Pos2, steps: usize, out: &mut Vec<Pos2>| {
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let u = 1.0 - t;
                    let x = u * u * p0.x + 2.0 * u * t * c.x + t * t * p1.x;
                    let y = u * u * p0.y + 2.0 * u * t * c.y + t * t * p1.y;
                    out.push(pos2(x, y));
                }
            };
            let mut a1 = Vec::new();
            quad(
                pos2(r.left() + s * 0.2, c.y + s * 0.1),
                pos2(r.left() + s * 0.45, c.y - s * 0.42),
                pos2(r.right() - s * 0.2, c.y + s * 0.1),
                24,
                &mut a1,
            );
            painter.add(egui::Shape::line(a1, stroke));
            let mut a2 = Vec::new();
            quad(
                pos2(r.right() - s * 0.2, c.y - s * 0.1),
                pos2(r.right() - s * 0.45, c.y + s * 0.42),
                pos2(r.left() + s * 0.2, c.y - s * 0.1),
                24,
                &mut a2,
            );
            painter.add(egui::Shape::line(a2, stroke));
        }
        "trash" => {
            // Trash can: lid, body and two vertical lines.
            let lid = Rect::from_min_max(
                pos2(r.left() + s * 0.08, r.top() + s * 0.28),
                pos2(r.right() - s * 0.08, r.top() + s * 0.4),
            );
            painter.rect_stroke(lid, CornerRadius::same(2), stroke, StrokeKind::Inside);
            let body = Rect::from_min_max(
                pos2(r.left() + s * 0.18, r.top() + s * 0.38),
                pos2(r.right() - s * 0.18, r.bottom() - s * 0.04),
            );
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            line(
                pos2(r.left() + s * 0.24, r.top() + s * 0.32),
                pos2(r.left() + s * 0.24, r.bottom() - s * 0.08),
            );
            line(
                pos2(r.right() - s * 0.24, r.top() + s * 0.32),
                pos2(r.right() - s * 0.24, r.bottom() - s * 0.08),
            );
        }
        "list" => {
            for i in 0..3 {
                let y = r.top() + s * (0.22 + i as f32 * 0.28);
                painter.circle_filled(pos2(r.left() + s * 0.1, y), s * 0.035, color);
                line(pos2(r.left() + s * 0.22, y), pos2(r.right() - s * 0.02, y));
            }
        }
        "globe" => {
            painter.circle_stroke(c, s * 0.42, stroke);
            line(pos2(r.left() + s * 0.1, c.y), pos2(r.right() - s * 0.1, c.y));
            line(pos2(c.x, r.top() + s * 0.08), pos2(c.x, r.bottom() - s * 0.08));
            let arc = Rect::from_center_size(c, vec2(s * 0.46, s * 0.82));
            painter.add(egui::Shape::line(
                vec![
                    pos2(arc.left(), arc.top() + arc.height() * 0.2),
                    pos2(arc.left() + s * 0.04, c.y),
                    pos2(arc.left(), arc.bottom() - arc.height() * 0.2),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![
                    pos2(arc.right(), arc.top() + arc.height() * 0.2),
                    pos2(arc.right() - s * 0.04, c.y),
                    pos2(arc.right(), arc.bottom() - arc.height() * 0.2),
                ],
                stroke,
            ));
        }
        "gear" | "settings" => {
            painter.circle_stroke(c, s * 0.25, stroke);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let inner = s * 0.33;
                let outer = s * 0.45;
                line(
                    pos2(c.x + a.cos() * inner, c.y + a.sin() * inner),
                    pos2(c.x + a.cos() * outer, c.y + a.sin() * outer),
                );
            }
        }
        "sun" => {
            painter.circle_stroke(c, s * 0.22, stroke);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                line(
                    pos2(c.x + a.cos() * s * 0.32, c.y + a.sin() * s * 0.32),
                    pos2(c.x + a.cos() * s * 0.45, c.y + a.sin() * s * 0.45),
                );
            }
        }
        "moon" => {
            let mut points = Vec::with_capacity(18);
            for i in 0..=16 {
                let a = std::f32::consts::PI * (0.25 + 1.5 * i as f32 / 16.0);
                points.push(pos2(c.x + a.cos() * s * 0.34, c.y + a.sin() * s * 0.34));
            }
            painter.add(egui::Shape::line(points, Stroke::new(width * 2.2, color)));
        }
        "search" => {
            painter.circle_stroke(pos2(c.x - s * 0.08, c.y - s * 0.08), s * 0.26, stroke);
            line(
                pos2(c.x + s * 0.1, c.y + s * 0.1),
                pos2(r.right() - s * 0.05, r.bottom() - s * 0.05),
            );
        }
        "folder" => {
            let tab = Rect::from_min_max(
                pos2(r.left() + s * 0.06, r.top() + s * 0.2),
                pos2(r.left() + s * 0.42, r.top() + s * 0.34),
            );
            let body = Rect::from_min_max(
                pos2(r.left() + s * 0.06, r.top() + s * 0.3),
                pos2(r.right() - s * 0.06, r.bottom() - s * 0.12),
            );
            painter.rect_filled(tab, CornerRadius::same(1), color);
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            line(
                pos2(body.left(), body.top()),
                pos2(tab.right(), body.top()),
            );
        }
        "file" => {
            let body = Rect::from_min_max(
                pos2(r.left() + s * 0.2, r.top() + s * 0.06),
                pos2(r.right() - s * 0.18, r.bottom() - s * 0.06),
            );
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            line(
                pos2(body.right() - s * 0.18, body.top()),
                pos2(body.right(), body.top() + s * 0.18),
            );
            line(
                pos2(body.right() - s * 0.18, body.top()),
                pos2(body.right() - s * 0.18, body.top() + s * 0.18),
            );
            line(
                pos2(body.right() - s * 0.18, body.top() + s * 0.18),
                pos2(body.right(), body.top() + s * 0.18),
            );
        }
        "info" => {
            painter.circle_stroke(c, s * 0.42, stroke);
            painter.circle_filled(pos2(c.x, c.y - s * 0.2), s * 0.045, color);
            line(pos2(c.x, c.y - s * 0.04), pos2(c.x, c.y + s * 0.24));
        }
        "x" => {
            line(
                pos2(c.x - s * 0.24, c.y - s * 0.24),
                pos2(c.x + s * 0.24, c.y + s * 0.24),
            );
            line(
                pos2(c.x + s * 0.24, c.y - s * 0.24),
                pos2(c.x - s * 0.24, c.y + s * 0.24),
            );
        }
        "chevron-left" => {
            line(pos2(c.x + s * 0.12, c.y - s * 0.24), pos2(c.x - s * 0.12, c.y));
            line(pos2(c.x - s * 0.12, c.y), pos2(c.x + s * 0.12, c.y + s * 0.24));
        }
        "chevron-right" => {
            line(pos2(c.x - s * 0.12, c.y - s * 0.24), pos2(c.x + s * 0.12, c.y));
            line(pos2(c.x + s * 0.12, c.y), pos2(c.x - s * 0.12, c.y + s * 0.24));
        }
        "chevron-up" | "up" => {
            line(pos2(c.x - s * 0.24, c.y + s * 0.12), pos2(c.x, c.y - s * 0.12));
            line(pos2(c.x, c.y - s * 0.12), pos2(c.x + s * 0.24, c.y + s * 0.12));
        }
        "chevron-down" | "down" => {
            line(pos2(c.x - s * 0.24, c.y - s * 0.12), pos2(c.x, c.y + s * 0.12));
            line(pos2(c.x, c.y + s * 0.12), pos2(c.x + s * 0.24, c.y - s * 0.12));
        }
        "panel-left" => {
            let body = r.shrink(s * 0.08);
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            line(
                pos2(body.left() + body.width() * 0.36, body.top()),
                pos2(body.left() + body.width() * 0.36, body.bottom()),
            );
        }
        "panel-right" => {
            let body = r.shrink(s * 0.08);
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            line(
                pos2(body.right() - body.width() * 0.36, body.top()),
                pos2(body.right() - body.width() * 0.36, body.bottom()),
            );
        }
        "play" => {
            poly(vec![
                pos2(c.x - s * 0.18, c.y - s * 0.28),
                pos2(c.x + s * 0.3, c.y),
                pos2(c.x - s * 0.18, c.y + s * 0.28),
            ]);
        }
        "pause" => {
            let w = s * 0.1;
            painter.rect_filled(
                Rect::from_center_size(c + vec2(-s * 0.13, 0.0), vec2(w, s * 0.44)),
                CornerRadius::same(1),
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(c + vec2(s * 0.13, 0.0), vec2(w, s * 0.44)),
                CornerRadius::same(1),
                color,
            );
        }
        "stop" => {
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::splat(s * 0.36)),
                CornerRadius::same(2),
                color,
            );
        }
        "prev" => {
            line(pos2(c.x - s * 0.25, c.y - s * 0.26), pos2(c.x - s * 0.25, c.y + s * 0.26));
            poly(vec![
                pos2(c.x + s * 0.25, c.y - s * 0.26),
                pos2(c.x - s * 0.1, c.y),
                pos2(c.x + s * 0.25, c.y + s * 0.26),
            ]);
        }
        "next" => {
            line(pos2(c.x + s * 0.25, c.y - s * 0.26), pos2(c.x + s * 0.25, c.y + s * 0.26));
            poly(vec![
                pos2(c.x - s * 0.25, c.y - s * 0.26),
                pos2(c.x + s * 0.1, c.y),
                pos2(c.x - s * 0.25, c.y + s * 0.26),
            ]);
        }
        "plus" => {
            line(pos2(c.x - s * 0.24, c.y), pos2(c.x + s * 0.24, c.y));
            line(pos2(c.x, c.y - s * 0.24), pos2(c.x, c.y + s * 0.24));
        }
        "minus" => {
            line(pos2(c.x - s * 0.24, c.y), pos2(c.x + s * 0.24, c.y));
        }
        "clock" => {
            painter.circle_stroke(c, s * 0.42, stroke);
            line(c, pos2(c.x, c.y - s * 0.24));
            line(c, pos2(c.x + s * 0.16, c.y + s * 0.12));
        }
        "chip" => {
            let body = Rect::from_center_size(c, vec2(s * 0.5, s * 0.5));
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            for i in 0..3 {
                let y = body.top() + body.height() * (i as f32 + 1.0) / 4.0;
                line(pos2(body.left() - s * 0.08, y), pos2(body.left(), y));
                line(pos2(body.right(), y), pos2(body.right() + s * 0.08, y));
            }
        }
        "speaker" => {
            poly(vec![
                pos2(r.left() + s * 0.14, c.y - s * 0.16),
                pos2(r.left() + s * 0.34, c.y - s * 0.16),
                pos2(r.left() + s * 0.52, c.y - s * 0.34),
                pos2(r.left() + s * 0.52, c.y + s * 0.34),
                pos2(r.left() + s * 0.34, c.y + s * 0.16),
                pos2(r.left() + s * 0.14, c.y + s * 0.16),
            ]);
            painter.add(egui::Shape::line(
                vec![
                    pos2(r.left() + s * 0.64, c.y - s * 0.18),
                    pos2(r.left() + s * 0.74, c.y),
                    pos2(r.left() + s * 0.64, c.y + s * 0.18),
                ],
                stroke,
            ));
        }
        "speaker-off" => {
            poly(vec![
                pos2(r.left() + s * 0.14, c.y - s * 0.16),
                pos2(r.left() + s * 0.34, c.y - s * 0.16),
                pos2(r.left() + s * 0.52, c.y - s * 0.34),
                pos2(r.left() + s * 0.52, c.y + s * 0.34),
                pos2(r.left() + s * 0.34, c.y + s * 0.16),
                pos2(r.left() + s * 0.14, c.y + s * 0.16),
            ]);
            line(
                pos2(r.left() + s * 0.62, c.y - s * 0.16),
                pos2(r.left() + s * 0.84, c.y + s * 0.16),
            );
            line(
                pos2(r.left() + s * 0.84, c.y - s * 0.16),
                pos2(r.left() + s * 0.62, c.y + s * 0.16),
            );
        }
        "wave" => {
            let mut points = Vec::with_capacity(20);
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let x = r.left() + s * 0.06 + t * s * 0.88;
                let y = c.y - (t * std::f32::consts::TAU * 1.5).sin() * s * 0.26;
                points.push(pos2(x, y));
            }
            painter.add(egui::Shape::line(points, stroke));
        }
        "refresh" => {
            painter.circle_stroke(c, s * 0.34, stroke);
            line(pos2(r.right() - s * 0.2, r.top() + s * 0.1), pos2(r.right() - s * 0.12, r.top() + s * 0.26));
            line(pos2(r.right() - s * 0.2, r.top() + s * 0.1), pos2(r.right() - s * 0.36, r.top() + s * 0.14));
        }
        _ => {
            painter.circle_stroke(c, s * 0.32, stroke);
        }
    }
}
