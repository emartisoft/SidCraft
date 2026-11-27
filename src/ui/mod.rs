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

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Layout, Margin, Pos2, Rect,
    Response, RichText, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use residfp::ChipModel;

use crate::engine::export::ExportFormat;
use crate::engine::player::{lock_player, try_lock_player};
use crate::engine::playlist::playlist_display_name;
use crate::theme::{self, AccentColor, ThemeKind, ThemePreference, Tokens};
use crate::visuals::SPECTROGRAM_COLUMNS;
use crate::{
    ConfirmAction, PlaylistDownloadProgress, SidcraftApp, UpdatePhase, View, VizMode,
    playlist_rows_for_query,
};

mod dialogs;
mod icons;
mod visualizers;
mod widgets;

use icons::*;
use widgets::*;


impl eframe::App for SidcraftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // winit rewrites the extended window style when it applies viewport
        // commands, which drops WS_EX_LAYERED. Re-assert the OS-level alpha on
        // every mini frame so the opacity setting cannot be undone by that.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = if self.mini_mode {
                (self.mini_opacity * 255.0).round() as u8
            } else {
                255
            };
            if self.mini_mode || self.last_window_alpha != Some(target) {
                self.apply_window_alpha(target, self.last_window_alpha);
                self.last_window_alpha = Some(target);
            }
        }

        self.handle_open_requests(ctx);
        self.handle_dropped_files(ctx);
        self.handle_async_hvsc();
        self.handle_csdb_async();
        self.handle_playlist_download_progress();
        self.handle_seek();
        self.handle_update_async();
        self.maybe_auto_check_updates();

        // Pull the emulated voice state once per frame for the meters and scopes.
        // `try_lock` matters while a seek is fast-forwarding in the background:
        // blocking here would freeze the whole UI.
        let viz_mode = self.viz_mode;
        let mut player_error: Option<String> = None;
        let mut player_paused = false;
        if let Some(mut p) = try_lock_player(&self.player) {
            player_error = p.take_error();
            player_paused = p.is_paused();
            p.copy_voice_levels_into(&mut self.voice_levels_buf);
            self.vu_meter.update(&self.voice_levels_buf);
            p.copy_voice_mutes_into(&mut self.voice_mutes);
            p.copy_envelope_samples_into(&mut self.envelope_curves);
            self.voice_scopes.update(&self.envelope_curves);
            match viz_mode {
                VizMode::Spectrum | VizMode::Spectrogram | VizMode::Oscilloscope => {
                    p.copy_audio_samples_into(&mut self.audio_samples);
                }
                VizMode::PianoRoll => {
                    p.copy_note_history_into(&mut self.note_history);
                }
                VizMode::Filter => {
                    p.copy_voice_infos_into(&mut self.voice_infos);
                    p.copy_filter_infos_into(&mut self.filter_infos);
                }
                VizMode::Adsr => {
                    p.copy_envelope_states_into(&mut self.envelope_states);
                }
                VizMode::Scope | VizMode::Vu => {}
            }
        }
        if let Some(err) = player_error {
            self.paused = true;
            self.set_status(err);
        } else if player_paused && !self.paused {
            self.paused = true;
        }

        // The FFT only runs while one of the spectrum views is actually visible.
        let viz_visible = self.mini_mode || self.view == View::Player;
        if viz_visible && matches!(viz_mode, VizMode::Spectrum | VizMode::Spectrogram) {
            self.spectrum.update(&self.audio_samples, self.sample_rate);
            if viz_mode == VizMode::Spectrogram {
                self.spectrogram.push(&self.spectrum.bands);
                self.spectrogram_dirty = true;
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
                && !self.song_timeout.is_zero()
                && elapsed >= self.song_timeout
            {
                self.handle_track_end();
            }
        }

        // Resolve the saved system/light/dark preference and accent hue, then
        // install the matching design tokens and egui style for this frame.
        let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark);
        let kind = self.theme_pref.resolve(system_dark, ThemeKind::Light);
        let applied = Some((kind, self.accent));
        if self.applied_theme != applied {
            theme::apply(ctx, kind, self.accent);
            self.applied_theme = applied;
        }

        // Do this once the first viewport info is available, so the full-shell
        // geometry stored for the later exit is the real one, not a fallback.
        if self.mini_start_pending {
            self.mini_start_pending = false;
            self.enter_mini_mode(ctx);
        }

        self.keyboard_shortcuts(ctx);
        self.sync_config();
        self.remember_window(ctx);

        // Keep the OS title bar in step with the loaded tune without allocating per frame.
        let title_matches = if self.sid_file.name.is_empty() {
            self.window_title == "SidCraft"
        } else if self.sid_file.author.is_empty() {
            self.window_title.len() == self.sid_file.name.len() + " - SidCraft".len()
                && self.window_title.starts_with(&self.sid_file.name)
                && self.window_title.ends_with(" - SidCraft")
        } else {
            self.window_title.len()
                == self.sid_file.author.len() + self.sid_file.name.len() + " -  - SidCraft".len()
                && self.window_title.starts_with(&self.sid_file.author)
                && self.window_title.ends_with(" - SidCraft")
                && self.window_title[self.sid_file.author.len() + 3..]
                    .starts_with(&self.sid_file.name)
        };
        if !title_matches {
            let title = if self.sid_file.name.is_empty() {
                "SidCraft".to_string()
            } else if self.sid_file.author.is_empty() {
                format!("{} - SidCraft", self.sid_file.name)
            } else {
                format!("{} - {} - SidCraft", self.sid_file.author, self.sid_file.name)
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        if self.mini_mode {
            self.mini_player(ctx);
        } else {
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
            if self.show_hvsc_mirror_editor {
                self.hvsc_mirror_window(ctx);
            }
            if self.csdb_releases.is_some() {
                self.csdb_releases_window(ctx);
            }
            if self.confirm_action.is_some() {
                self.confirm_window(ctx);
            }
            if self.random_list_result.is_some() {
                self.random_list_result_window(ctx);
            }
            if self.show_update_dialog {
                self.update_window(ctx);
            }
        }

        // Drag-and-drop results are shown in both the full shell and the mini
        // player, since a drop can happen in either window.
        if self.drop_report.is_some() {
            self.drop_report_window(ctx);
        }
        if !matches!(self.playlist_download, PlaylistDownloadProgress::Idle) {
            self.playlist_download_window(ctx);
        }

        // Repaint only as fast as needed: real-time visuals/timing while playing
        // or seeking, a precise wake-up for the inter-track gap, otherwise idle.
        if let Some(until) = self.track_gap_until {
            let wait = until.saturating_duration_since(Instant::now());
            ctx.request_repaint_after(wait);
        } else if (!self.paused && self.sid_file.name.is_empty() == false) || self.seeking {
            // ~40 FPS for smooth meters, scopes and elapsed time while playing.
            ctx.request_repaint_after(Duration::from_millis(25));
        } else if self.hvsc_loading
            || self.csdb_releases.as_ref().is_some_and(|dialog| dialog.loading)
            || self.export_progress.is_some()
            || self.update_busy()
            || matches!(
                self.playlist_download,
                PlaylistDownloadProgress::Running { .. }
            )
        {
            // Keep the loading state responsive without running at full rate.
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.save();
    }

    /// Renderer-level transparency. The viewport is created with an alpha
    /// channel, so the mini player's clear colour can be faded to let the
    /// desktop show through; the full shell clears fully opaque.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let alpha = if self.mini_mode { self.mini_opacity } else { 1.0 };
        egui::Color32::from_rgba_unmultiplied(
            12,
            12,
            12,
            (alpha * 255.0).round() as u8,
        )
        .to_normalized_gamma_f32()
    }
}

impl SidcraftApp {
    // ── mini player ─────────────────────────────────────────────────────────

    /// Draws the compact frameless player. The OS title bar is removed by
    /// `enter_mini_mode`; the header strip drags the window and the button in
    /// its corner returns to the full shell.
    fn mini_player(&mut self, ctx: &egui::Context) {
        // Apply the opacity to the local token copy only; the rest of the
        // shell keeps its normal palette.
        let alpha = (self.mini_opacity * 255.0).round() as u8;
        let t = Tokens::get(ctx).with_opacity(alpha);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .inner_margin(Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                let content_h = ui.available_height();

                // ── title block, also the window drag handle ────────────────
                let (header_rect, header) = ui.allocate_exact_size(
                    vec2(ui.available_width(), 44.0),
                    Sense::click_and_drag(),
                );
                let expand_rect = Rect::from_min_size(
                    pos2(header_rect.right() - 26.0, header_rect.top() + 1.0),
                    Vec2::splat(24.0),
                );
                let pointer = ui.ctx().pointer_latest_pos();
                let over_expand = pointer.is_some_and(|p| expand_rect.contains(p));
                let text_w = (header_rect.width() - 32.0).max(1.0);

                let title = self.track_title();
                paint_text_elided(
                    ui,
                    pos2(header_rect.left(), header_rect.top() + 2.0),
                    Align2::LEFT_TOP,
                    &title,
                    theme::semibold(14.5),
                    t.text,
                    text_w,
                );
                let author = if self.sid_file.author.is_empty() {
                    "Unknown author".to_string()
                } else if self.sid_file.released.is_empty() {
                    self.sid_file.author.clone()
                } else {
                    format!("{}  ·  {}", self.sid_file.author, self.sid_file.released)
                };
                paint_text_elided(
                    ui,
                    pos2(header_rect.left(), header_rect.top() + 23.0),
                    Align2::LEFT_TOP,
                    &author,
                    theme::regular(11.5),
                    t.text_muted,
                    text_w,
                );

                let expand = {
                    let resp =
                        ui.interact(expand_rect, ui.id().with("mini-expand"), Sense::click());
                    if resp.hovered() {
                        ui.painter()
                            .rect_filled(expand_rect, CornerRadius::same(6), t.hover);
                    }
                    paint_icon(
                        ui.painter(),
                        expand_rect.shrink(5.0),
                        "maximize",
                        if resp.hovered() { t.accent } else { t.text_muted },
                        1.5,
                    );
                    theme::hand(resp)
                        .on_hover_text("Exit mini player")
                        .clicked()
                };

                if header.drag_started() && !over_expand {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if header.double_clicked() && !over_expand {
                    self.exit_mini_mode(ctx);
                    return;
                }
                if header.hovered() && !over_expand {
                    ui.ctx().set_cursor_icon(CursorIcon::Grab);
                }
                if expand {
                    self.exit_mini_mode(ctx);
                    return;
                }

                // ── visualiser, between the track info and the buttons ─────
                // The full player's display has a large clock and ticker; the
                // compact variant draws only the selected visualiser in a
                // small framed area that fits the remaining space.
                const TRANSPORT_H: f32 = 40.0;
                const ROW_GAP: f32 = 4.0;
                let viz_h = (content_h - 44.0 - TRANSPORT_H - ROW_GAP * 2.0).max(60.0);
                ui.add_space(ROW_GAP);
                let (viz_rect, viz) =
                    ui.allocate_exact_size(vec2(ui.available_width(), viz_h), Sense::click());
                match self.viz_mode {
                    VizMode::Scope => self.paint_scope(ui, &t, viz_rect, true),
                    VizMode::Vu => self.paint_display_vu(ui, &t, viz_rect, true),
                    VizMode::Spectrum => self.paint_spectrum(ui, &t, viz_rect, true),
                    VizMode::Spectrogram => self.paint_spectrogram(ui, &t, viz_rect, true),
                    VizMode::Oscilloscope => self.paint_oscilloscope(ui, &t, viz_rect, true),
                    VizMode::PianoRoll => self.paint_piano_roll(ui, &t, viz_rect, true),
                    VizMode::Filter => self.paint_filter(ui, &t, viz_rect, true),
                    VizMode::Adsr => self.paint_adsr(ui, &t, viz_rect, true),
                }
                // Mini-only shortcut: double-click the visualiser to cycle to
                // the next one, without opening the full shell's selector.
                if self.mini_mode && viz.double_clicked() {
                    let next = VizMode::ALL
                        .iter()
                        .position(|mode| *mode == self.viz_mode)
                        .map_or(0, |i| (i + 1) % VizMode::ALL.len());
                    self.viz_mode = VizMode::ALL[next];
                }
                let _ = viz.on_hover_text("Double-click for next visualisation");

                // ── transport and seek bar share the same row ──────────────
                ui.add_space(ROW_GAP);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), TRANSPORT_H),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        const PREV_SIZE: f32 = 30.0;
                        const PLAY_SIZE: f32 = 40.0;
                        const NEXT_SIZE: f32 = 30.0;
                        const GAP: f32 = 10.0;
                        if transport_button(
                            ui,
                            &t,
                            "prev",
                            PREV_SIZE,
                            false,
                            "Previous track",
                        )
                        .clicked()
                        {
                            self.prev_track();
                        }
                        ui.add_space(GAP);
                        if transport_button(
                            ui,
                            &t,
                            if self.paused { "play" } else { "pause" },
                            PLAY_SIZE,
                            true,
                            "Play / pause",
                        )
                        .clicked()
                        {
                            self.toggle_pause();
                        }
                        ui.add_space(GAP);
                        if transport_button(ui, &t, "next", NEXT_SIZE, false, "Next track").clicked()
                        {
                            self.next_track();
                        }
                        // A short gap before the bar, then leave room in the
                        // corner for the resize grip.
                        ui.add_space(12.0);
                        let elapsed = self.current_elapsed();
                        let total = self.song_timeout;
                        let fraction =
                            (elapsed.as_secs_f32() / total.as_secs_f32().max(1.0)).clamp(0.0, 1.0);
                        let bar_w = (ui.available_width() - 18.0).max(40.0);
                        ui.allocate_ui_with_layout(
                            vec2(bar_w, TRANSPORT_H),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                if let Some(f) = seek_bar(ui, &t, fraction) {
                                    self.seek_to_fraction(f);
                                }
                            },
                        );
                    },
                );

                // Frameless windows have no native sizing border, so the OS
                // cannot resize them by dragging an edge. Put the grip in the
                // actual window corner; the panel margin clips interactions,
                // so widen the clip rect while the handle is registered.
                let screen = ctx.screen_rect();
                let grip_rect = Rect::from_min_size(
                    pos2(screen.right() - 18.0, screen.bottom() - 18.0),
                    Vec2::splat(18.0),
                );
                let clip = ui.clip_rect();
                ui.set_clip_rect(clip.expand(12.0));
                let grip = ui.interact(
                    grip_rect,
                    ui.id().with("mini-resize"),
                    Sense::click_and_drag(),
                );
                if grip.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(
                        egui::ResizeDirection::SouthEast,
                    ));
                }
                if grip.hovered() {
                    ctx.set_cursor_icon(CursorIcon::ResizeNwSe);
                }
                // Inset the strokes slightly so they stay visible at the corner.
                let anchor = pos2(grip_rect.right() - 6.0, grip_rect.bottom() - 6.0);
                let grip_color = if grip.hovered() { t.accent } else { t.text_faint };
                for i in 0..3 {
                    let len = 4.0 + i as f32 * 4.0;
                    ui.painter().line_segment(
                        [pos2(anchor.x - len, anchor.y), pos2(anchor.x, anchor.y - len)],
                        Stroke::new(1.5_f32, grip_color),
                    );
                }
                ui.set_clip_rect(clip);
            });
    }

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
        if ctx.input(|i| i.key_pressed(egui::Key::M) && i.modifiers.command) {
            if self.mini_mode {
                self.exit_mini_mode(ctx);
            } else {
                self.enter_mini_mode(ctx);
            }
        }
    }

    // ── mode bar ────────────────────────────────────────────────────────────

    fn mode_bar(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::TopBottomPanel::top("sidcraft_mode_bar")
            .exact_height(48.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.titlebar)
                    .inner_margin(Margin::symmetric(10, 0))
                    .stroke(Stroke::new(1.0_f32, t.divider)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    // Home sits just left of the menu: the shell has a single
                    // top bar, so view switching lives here.
                    if icon_button(ui, &t, "home", 30.0, self.view == View::Home, "Home")
                        .clicked()
                    {
                        self.view = View::Home;
                    }
                    ui.add_space(4.0);

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

                    for view in [View::Player, View::Playlist, View::Hvsc, View::RandomList, View::Export, View::Settings] {
                        if mode_tab(ui, &t, view.label(), self.view == view).clicked() {
                            self.view = view;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // The bar also holds the panel toggles, the theme
                        // switch and About, so let the search field shrink on
                        // narrow windows instead of pushing them out of view.
                        let search_width = (ui.available_width() - 200.0).clamp(110.0, 240.0);
                        if icon_button(ui, &t, "mini", 32.0, false, "Mini player (Ctrl+M)").clicked()
                        {
                            self.enter_mini_mode(ui.ctx());
                        }
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
                            search_width,
                            "Search HVSC tunes",
                        );
                        if search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.hvsc_search_query = self.global_search.clone();
                            self.view = View::Hvsc;
                            self.start_hvsc_search();
                        }
                        ui.add_space(4.0);
                        // Theme switch and About sit just left of the search
                        // field, where the removed tab strip used to keep them.
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
                        if icon_button(ui, &t, "info", 30.0, false, "About SidCraft").clicked() {
                            self.show_about = true;
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
                View::RandomList,
                View::Export,
                View::Settings,
            ] {
                if ui
                    .add(egui::Button::new(view.label()).selected(self.view == view))
                    .clicked()
                {
                    ui.close_menu();
                    self.view = view;
                    if view != View::Home {
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
            let busy = self.update_busy();
            let has_result = matches!(
                self.update_phase,
                UpdatePhase::Available | UpdatePhase::Ready(_)
            );
            let label = if has_result {
                "Show Update…"
            } else {
                "Check for Updates…"
            };
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                ui.close_menu();
                if has_result {
                    self.show_update_dialog = true;
                } else {
                    self.start_update_check(true);
                }
            }
            ui.separator();
            if ui.button("About SidCraft").clicked() {
                ui.close_menu();
                self.show_about = true;
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
                        }
                        let playlist_info = format!(
                            "{} - {} tracks",
                            playlist_display_name(&self.playlist_path),
                            self.playlist.len()
                        );
                        if tool_row(ui, &t, "list", "Playlist", &playlist_info, self.view == View::Playlist).clicked() {
                            self.view = View::Playlist;
                        }
                        if tool_row(ui, &t, "globe", "HVSC browser", "Search the High Voltage SID Collection", self.view == View::Hvsc).clicked() {
                            self.view = View::Hvsc;
                        }
                        if tool_row(ui, &t, "shuffle", "Random List", "Random playlists from HVSC collections", self.view == View::RandomList).clicked() {
                            self.view = View::RandomList;
                        }
                        if tool_row(ui, &t, "save", "Export", "Render to WAV, FLAC or MP3", self.view == View::Export).clicked() {
                            self.view = View::Export;
                        }
                        if tool_row(ui, &t, "gear", "Settings", "Appearance, audio and emulation", self.view == View::Settings).clicked() {
                            self.view = View::Settings;
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
                            if self.can_download_current_sid()
                                && icon_button_glyph(
                                    ui,
                                    &t,
                                    "download",
                                    height,
                                    (height * 0.8).min(21.0),
                                    1.6,
                                    false,
                                    "Download current SID file…",
                                )
                                .clicked()
                            {
                                self.download_current_sid();
                            }
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
                    }
                    if icon_button(ui, &t, "list", 34.0, self.view == View::Playlist, "Playlist").clicked() {
                        self.view = View::Playlist;
                    }
                    if icon_button(ui, &t, "globe", 34.0, self.view == View::Hvsc, "HVSC browser").clicked() {
                        self.view = View::Hvsc;
                    }
                    if icon_button(ui, &t, "shuffle", 34.0, self.view == View::RandomList, "Random List").clicked() {
                        self.view = View::RandomList;
                    }
                    if icon_button(ui, &t, "save", 34.0, self.view == View::Export, "Export").clicked() {
                        self.view = View::Export;
                    }
                    if icon_button(ui, &t, "gear", 34.0, self.view == View::Settings, "Settings").clicked() {
                        self.view = View::Settings;
                    }
                });

                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
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
                                let load = self.sid_file.load_address;
                                let init = self.sid_file.init_address;
                                let play = self.sid_file.play_address;
                                let size = self.sid_file.data.len();
                                let end = load
                                    .wrapping_add(size as u16)
                                    .wrapping_sub(u16::from(size != 0));
                                property_row_right(
                                    ui,
                                    &t,
                                    "Memory range",
                                    &format!("${load:04X}-${end:04X}, {size} bytes"),
                                );
                                property_row_right(
                                    ui,
                                    &t,
                                    "Load-Init-Play",
                                    &format!("${load:04X}-${init:04X}-${play:04X}"),
                                );
                                property_row_right(ui, &t, "Filter", if self.ekv_filter { "EKV on" } else { "Standard" });

                                if self.active_sid_hvsc_path().is_some() {
                                    ui.add_space(16.0);
                                    section_title(ui, &t, "Releases Using This SID");
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.add_space(14.0);
                                        let fetch = ui.add(
                                            egui::Label::new(
                                                RichText::new("Fetch Releases")
                                                    .font(theme::regular(12.0))
                                                    .color(t.accent_text)
                                                    .underline(),
                                            )
                                            .sense(egui::Sense::click()),
                                        );
                                        let fetch =
                                            fetch.on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if fetch.clicked() {
                                            self.fetch_sid_releases();
                                        }
                                    });
                                }

                                ui.add_space(16.0);
                                section_title(ui, &t, "Voices");
                                ui.add_space(4.0);
                                if let Some(voice) = voice_meters(
                                    ui,
                                    &t,
                                    &self.vu_meter.levels,
                                    &self.vu_meter.peaks,
                                    &self.voice_mutes,
                                ) {
                                    let muted =
                                        self.voice_mutes.get(voice).copied().unwrap_or(false);
                                    lock_player(&self.player).set_voice_muted(voice, !muted);
                                    if let Some(slot) = self.voice_mutes.get_mut(voice) {
                                        *slot = !muted;
                                    }
                                }
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
                View::RandomList => self.random_list_view(ui),
                View::Export => self.export_view(ui),
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
                                (View::RandomList, "shuffle", "Random List", "Build a random playlist from HVSC"),
                                (View::Export, "save", "Export", "Render to WAV, FLAC or MP3"),
                                (View::Settings, "gear", "Settings", "Theme, emulation and visualiser"),
                            ];
                            for (view, icon, title, blurb) in cards {
                                if tool_card(ui, &t, icon, title, blurb, t.voice(view as usize)).clicked() {
                                    self.view = view;
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
                                let row = playlist_row(ui, &t, idx, &name, &source, subsong, selected, false);
                                if row.double_clicked() {
                                    play = Some(idx);
                                }
                            }
                            if let Some(idx) = play {
                                self.play_playlist_index(idx);
                                self.sync_playlist_search_cursor();
                                // Deliberately stay on the current view: the
                                // playlist is the browser, not a launch pad
                                // that navigates away as soon as a tune plays.
                            }
                        }
                    });
            });
    }

    fn player_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        // Keep the card within a comfortable range while the window is
        // resized. Below the minimum the view scrolls horizontally instead of
        // squeezing the transport controls past usability.
        const CARD_MARGIN: f32 = 20.0;
        const MIN_PAGE_WIDTH: f32 = 480.0;
        const MAX_PAGE_WIDTH: f32 = 720.0;
        const SIDE_GUTTER: f32 = 24.0;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width();
                // Leave room for the frame margin and both gutters so the card
                // fits exactly whenever it isn't clamped to its minimum size.
                let page_width = (available - 2.0 * (SIDE_GUTTER + CARD_MARGIN))
                    .clamp(MIN_PAGE_WIDTH, MAX_PAGE_WIDTH);
                // Centre the card including its frame margin, so the gutters
                // stay equal on both sides as the window is resized.
                let card_width = page_width + 2.0 * CARD_MARGIN;
                let side_gutter = ((available - card_width) / 2.0).max(SIDE_GUTTER);

                // Mirror the horizontal gutters vertically: use the last
                // measured card height to keep the top and bottom gaps equal.
                let available_height = ui.available_height();
                let top_gutter = ((available_height - self.player_card_height) / 2.0).max(0.0);
                ui.add_space(top_gutter);
                let card_height = ui
                    .horizontal(|ui| {
                        ui.add_space(side_gutter);
                        let response = egui::Frame::NONE
                        .fill(t.card)
                        .stroke(Stroke::new(1.0_f32, t.border))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 3],
                            blur: 14,
                            spread: 0,
                            color: t.page_shadow,
                        })
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::same(CARD_MARGIN as i8))
                        .show(ui, |ui| {
                            // The frame inherits the surrounding horizontal
                            // layout; give the card its own vertical column.
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
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
                                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                        // The visualiser selector stays pinned
                                        // to the top-right of the header.
                                        ui.allocate_ui_with_layout(
                                            vec2(128.0, 0.0),
                                            Layout::top_down(Align::Max),
                                            |ui| {
                                                let mut mode = self.viz_mode;
                                                egui::ComboBox::from_id_salt("player-viz")
                                                    .width(128.0)
                                                    .selected_text(
                                                        RichText::new(mode.label())
                                                            .font(theme::medium(11.0)),
                                                    )
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
                                            },
                                        );
                                    });
                                });

                                ui.add_space(14.0);

                                let (display_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 276.0), Sense::hover());
                                match self.viz_mode {
                                    VizMode::Scope => self.paint_scope(ui, &t, display_rect, false),
                                    VizMode::Vu => self.paint_display_vu(ui, &t, display_rect, false),
                                    VizMode::Spectrum => self.paint_spectrum(ui, &t, display_rect, false),
                                    VizMode::Spectrogram => {
                                        self.paint_spectrogram(ui, &t, display_rect, false);
                                    }
                                    VizMode::Oscilloscope => {
                                        self.paint_oscilloscope(ui, &t, display_rect, false);
                                    }
                                    VizMode::PianoRoll => self.paint_piano_roll(ui, &t, display_rect, false),
                                    VizMode::Filter => self.paint_filter(ui, &t, display_rect, false),
                                    VizMode::Adsr => self.paint_adsr(ui, &t, display_rect, false),
                                }

                                ui.add_space(10.0);
                                let total = self.song_timeout.as_secs_f32().max(1.0);
                                let fraction = (self.current_elapsed().as_secs_f32() / total).clamp(0.0, 1.0);
                                if let Some(f) = seek_bar(ui, &t, fraction) {
                                    self.seek_to_fraction(f);
                                }
                                // Elapsed and total time sit at the edges, with the
                                // whole play/pause/stop/next/shuffle/repeat group
                                // centred between them.
                                let elapsed_text = format_duration(self.current_elapsed());
                                let total_text = format_duration(self.song_timeout);
                                let time_font = theme::mono(11.0);
                                let elapsed_w = text_width(ui, &elapsed_text, &time_font);
                                let total_w = text_width(ui, &total_text, &time_font);
                                const TRANSPORT_SIZE: f32 = 46.0;
                                // The play/pause button is the main action, so
                                // it is a little larger than its neighbours.
                                const PLAY_SIZE: f32 = 54.0;
                                const TOGGLE_SIZE: f32 = 34.0;
                                const TRANSPORT_GAP: f32 = 8.0;
                                let group_w = TRANSPORT_SIZE * 3.0
                                    + PLAY_SIZE
                                    + TOGGLE_SIZE * 2.0
                                    + TRANSPORT_GAP * 5.0;

                                // A fixed-height row keeps the time labels and the
                                // transport buttons on the same vertical centre.
                                ui.allocate_ui_with_layout(
                                    vec2(ui.available_width(), PLAY_SIZE),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        ui.add_sized(
                                            [elapsed_w, 18.0],
                                            egui::Label::new(
                                                RichText::new(elapsed_text)
                                                    .font(time_font.clone())
                                                    .color(t.text_muted),
                                            )
                                            .extend(),
                                        );

                                        let center_w = (ui.available_width() - total_w).max(0.0);
                                        ui.add_space(((center_w - group_w) / 2.0).max(0.0));
                                        // Keep the buttons as direct children of
                                        // this row so they share its vertical
                                        // centre with the time labels.
                                        if transport_button(ui, &t, "prev", TRANSPORT_SIZE, false, "Previous track").clicked() {
                                            self.prev_track();
                                        }
                                        ui.add_space(TRANSPORT_GAP);
                                        if transport_button(ui, &t, if self.paused { "play" } else { "pause" }, PLAY_SIZE, true, "Play / pause").clicked() {
                                            self.toggle_pause();
                                        }
                                        ui.add_space(TRANSPORT_GAP);
                                        if transport_button(ui, &t, "stop", TRANSPORT_SIZE, false, "Stop").clicked() {
                                            self.stop_playback();
                                        }
                                        ui.add_space(TRANSPORT_GAP);
                                        if transport_button(ui, &t, "next", TRANSPORT_SIZE, false, "Next track").clicked() {
                                            self.next_track();
                                        }
                                        ui.add_space(TRANSPORT_GAP);
                                        if icon_button_glyph(ui, &t, "shuffle", TOGGLE_SIZE, 20.0, 1.9, self.shuffle, "Shuffle").clicked() {
                                            self.shuffle = !self.shuffle;
                                        }
                                        ui.add_space(TRANSPORT_GAP);
                                        if icon_button_glyph(ui, &t, "repeat", TOGGLE_SIZE, 20.0, 1.9, self.repeat_mode.is_active(), "Repeat").clicked() {
                                            self.toggle_repeat();
                                        }
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.add_sized(
                                                [total_w, 18.0],
                                                egui::Label::new(
                                                    RichText::new(total_text)
                                                        .font(time_font)
                                                        .color(t.text_faint),
                                                )
                                                .extend(),
                                            );
                                        });
                                    },
                                );

                                // Subsong stepping and volume live on their own row
                                // below the transport controls. The row wraps on
                                // narrow windows instead of letting the right-hand
                                // group overlap the subsong buttons.
                                ui.add_space(8.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    // The chip-model button sits to the left of
                                    // the subsong stepper.
                                    if secondary_button_with_font(
                                        ui,
                                        &t,
                                        self.chip_label(),
                                        theme::medium(11.0),
                                    )
                                    .clicked()
                                    {
                                        self.cycle_chip_model();
                                    }
                                    // Filter-model toggle: "EKV" when the
                                    // transistor model is on, "STD" for the
                                    // standard spline filter. The choice is a
                                    // persisted preference.
                                    let filter_label =
                                        if self.ekv_filter { "EKV" } else { "STD" };
                                    if secondary_button_with_font(
                                        ui,
                                        &t,
                                        filter_label,
                                        theme::medium(11.0),
                                    )
                                    .on_hover_text(
                                        "Filter model: EKV transistor (6581) / standard",
                                    )
                                    .clicked()
                                    {
                                        self.set_ekv_filter(!self.ekv_filter);
                                    }
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

                                    // Keep the volume/mute/download/add group on
                                    // this row. The volume bar takes whatever is
                                    // left after the three fixed 40 px buttons,
                                    // so the add button never wraps on its own.
                                    let spacing = ui.spacing().item_spacing.x;
                                    // `available_width` reports the whole row in a
                                    // wrapped layout, so use the cursor-relative
                                    // rect for the space left on this row.
                                    let remaining = ui.available_rect_before_wrap().width();
                                    let fixed_width = 3.0 * 40.0 + 4.0 * spacing;
                                    let volume_width =
                                        110.0_f32.min((remaining - fixed_width).max(44.0));
                                    ui.add_space(
                                        (remaining - fixed_width - volume_width).max(0.0),
                                    );

                                    let muted =
                                        self.muted.load(std::sync::atomic::Ordering::SeqCst);
                                    let frac = if muted {
                                        0.0
                                    } else {
                                        *self
                                            .volume
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner())
                                    };
                                    if let Some(f) = volume_bar(ui, &t, frac, volume_width) {
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
                                    if self.can_download_current_sid()
                                        && icon_button_glyph(
                                            ui,
                                            &t,
                                            "download",
                                            40.0,
                                            21.0,
                                            1.6,
                                            false,
                                            "Download current SID file…",
                                        )
                                        .clicked()
                                    {
                                        self.download_current_sid();
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
                                });
                            });
                        })
                        .response;
                    ui.add_space(side_gutter);
                    response.rect.height()
                })
                .inner;
                self.player_card_height = card_height;
                ui.add_space(top_gutter);
            });
    }

    /// Shared LCD chrome: dark panel, status strip and bottom ticker.
    /// Returns the content rectangle the visualisers draw in.
    fn playlist_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        // A drag whose release happened while another view was open must not
        // come back as a phantom reorder the next time this view is shown.
        if self.playlist_drag_from.is_some()
            && !ui.input(|i| i.pointer.primary_down())
            && !ui.input(|i| i.pointer.primary_released())
        {
            self.playlist_drag_from = None;
        }
        egui::Frame::NONE
            .inner_margin(Margin {
                left: 28,
                right: 28,
                top: 24,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let file_count = self.playlist_files.len();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Playlist")
                            .font(theme::semibold(22.0))
                            .color(t.text),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            file_count,
                            if file_count == 1 {
                                "playlist"
                            } else {
                                "playlists"
                            }
                        ))
                        .font(theme::regular(11.5))
                        .color(t.text_faint),
                    );
                    if icon_button(ui, &t, "folder-open", 34.0, false, "Open playlist folder")
                        .clicked()
                    {
                        self.open_playlist_folder();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Library")
                            .font(theme::medium(11.5))
                            .color(t.text_muted),
                    );

                    let current = playlist_display_name(&self.playlist_path);
                    let mut chosen: Option<std::path::PathBuf> = None;
                    egui::ComboBox::from_id_salt("playlist_library")
                        .selected_text(
                            RichText::new(current).font(theme::regular(12.0)),
                        )
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            if self.playlist_files.is_empty() {
                                ui.label(
                                    RichText::new("No playlists yet")
                                        .font(theme::regular(11.5))
                                        .color(t.text_muted),
                                );
                            }
                            for path in &self.playlist_files {
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
                    if icon_button(ui, &t, "plus", 34.0, false, "New playlist…").clicked() {
                        self.show_new_playlist = true;
                        self.new_playlist_name.clear();
                        self.new_playlist_error = None;
                    }
                    if icon_button(ui, &t, "folder", 34.0, false, "Open playlist or SID…")
                        .clicked()
                    {
                        self.open_file_dialog();
                    }
                    if icon_button(ui, &t, "pencil", 34.0, false, "Rename playlist").clicked() {
                        self.rename_playlist_name = playlist_display_name(&self.playlist_path);
                        self.rename_playlist_error = None;
                        self.show_rename_playlist = true;
                    }
                    if icon_button(ui, &t, "trash", 34.0, false, "Delete playlist").clicked() {
                        self.confirm_action = Some(ConfirmAction::DeletePlaylist);
                    }
                    if icon_button(ui, &t, "save", 34.0, false, "Save playlist as…").clicked() {
                        self.save_playlist_as();
                    }
                    if icon_button(
                        ui,
                        &t,
                        "download",
                        34.0,
                        false,
                        "Download all playlist SID files…",
                    )
                    .clicked()
                    {
                        self.download_all_playlist_sids();
                    }
                    if icon_button(ui, &t, "play", 34.0, false, "Play all").clicked() {
                        self.play_all();
                        self.view = View::Player;
                    }
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
                let track_count = self.playlist.len();
                ui.horizontal_wrapped(|ui| {
                    if primary_icon_button(ui, &t, "file-plus", 34.0, "Add files").clicked() {
                        self.add_files_dialog();
                    }
                    if icon_button(ui, &t, "eraser", 34.0, false, "Clear playlist").clicked() {
                        self.confirm_action = Some(ConfirmAction::ClearTracks);
                    }
                    if icon_button(ui, &t, "minus", 34.0, false, "Remove selected track")
                        .clicked()
                    {
                        self.confirm_action = Some(ConfirmAction::RemoveTrack);
                    }
                    let has_selection = self.selected_playlist_idx < track_count;
                    ui.add_space(6.0);
                    if icon_button_enabled(
                        ui,
                        &t,
                        "download",
                        34.0,
                        false,
                        has_selection,
                        "Download selected SID file…",
                    )
                    .clicked()
                        && has_selection
                    {
                        self.download_selected_playlist_sid();
                    }
                });

                // Track reordering and search share the row below the actions.
                ui.add_space(6.0);
                let mut matches = Vec::new();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Move Track")
                            .font(theme::semibold(11.5))
                            .color(t.text_muted),
                    );
                    ui.add_space(2.0);
                    let selected = self.selected_playlist_idx;
                    let can_move_up = selected > 0 && selected < track_count;
                    let can_move_down = selected + 1 < track_count;
                    if icon_button_enabled(
                        ui,
                        &t,
                        "up",
                        34.0,
                        false,
                        can_move_up,
                        "Move track up",
                    )
                    .clicked()
                        && can_move_up
                    {
                        self.move_selected_track_up();
                    }
                    if icon_button_enabled(
                        ui,
                        &t,
                        "down",
                        34.0,
                        false,
                        can_move_down,
                        "Move track down",
                    )
                    .clicked()
                        && can_move_down
                    {
                        self.move_selected_track_down();
                    }
                    if icon_button_enabled(
                        ui,
                        &t,
                        "to-top",
                        34.0,
                        false,
                        can_move_up,
                        "Move to top",
                    )
                    .clicked()
                        && can_move_up
                    {
                        self.move_selected_track_to_top();
                    }
                    if icon_button_enabled(
                        ui,
                        &t,
                        "to-bottom",
                        34.0,
                        false,
                        can_move_down,
                        "Move to bottom",
                    )
                    .clicked()
                        && can_move_down
                    {
                        self.move_selected_track_to_bottom();
                    }

                    // A 3-4 item-spacing gap separates move from search.
                    ui.add_space(ui.spacing().item_spacing.x * 4.0);

                    let mut query = self.playlist_search.clone();
                    search_box(ui, &t, &mut query, 170.0, "Search tracks…");
                    if query != self.playlist_search {
                        self.playlist_search = query;
                        self.playlist_drag_from = None;
                        self.goto_playlist_search_match(0);
                    }
                    matches = self.playlist_search_matches().to_vec();
                    if !self.playlist_search.trim().is_empty() {
                        let count = matches.len();
                        let cursor = self.playlist_search_match.min(count.saturating_sub(1));
                        let summary = if count == 0 {
                            "No matches".to_string()
                        } else {
                            format!("{}/{}", cursor + 1, count)
                        };
                        ui.label(
                            RichText::new(summary)
                                .font(theme::regular(11.0))
                                .color(t.text_muted),
                        );
                        let can_prev = count > 0 && cursor > 0;
                        let can_next = count > 0 && cursor + 1 < count;
                        if icon_button_enabled(
                            ui, &t, "up", 26.0, false, can_prev, "Previous match",
                        )
                        .clicked()
                            && can_prev
                        {
                            self.goto_playlist_search_match(cursor - 1);
                        }
                        if icon_button_enabled(
                            ui, &t, "down", 26.0, false, can_next, "Next match",
                        )
                        .clicked()
                            && can_next
                        {
                            self.goto_playlist_search_match(cursor + 1);
                        }
                    }
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                let mut play = None;
                let mut select = None;
                let total_rows = self.playlist.entries.len();
                let visible_rows = playlist_rows_for_query(total_rows, &self.playlist_search, &matches);
                let visible_count = visible_rows.len();
                let filtering = !self.playlist_search.trim().is_empty();
                let scroll_to_selected = self.playlist_scroll_to_selected;
                let selected_idx = self.selected_playlist_idx;
                let mut drag_started: Option<usize> = None;
                let mut drag_from: Option<usize> = None;
                let mut drop_insert: Option<usize> = None;
                let mut row_rects: Vec<(usize, Rect)> = Vec::new();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    // Dragging a row reorders it; it must not scroll the list.
                    .drag_to_scroll(false)
                    .show_rows(ui, 48.0, visible_count, |ui, row_range| {
                        if visible_count == 0 {
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new(if filtering {
                                    "No tracks match the current search."
                                } else {
                                    "This playlist is empty. Use Add files, New…, or the HVSC browser."
                                })
                                .font(theme::regular(13.0))
                                .color(t.text_muted),
                            );
                        }
                        let mut selected_rect: Option<Rect> = None;
                        for visible_row in row_range {
                            let idx = visible_rows[visible_row];
                            let entry = &self.playlist.entries[idx];
                            let selected = idx == selected_idx;
                            let row = playlist_row(
                                ui,
                                &t,
                                idx,
                                &entry.display_name,
                                &entry.source,
                                entry.subsong,
                                selected,
                                !filtering,
                            );
                            row_rects.push((idx, row.rect));
                            if selected {
                                selected_rect = Some(row.rect);
                            }
                            if !filtering && row.drag_started() {
                                drag_started = Some(idx);
                            }
                            if row.clicked() {
                                select = Some(idx);
                            }
                            if row.double_clicked() {
                                play = Some(idx);
                            }
                        }

                        // The drag source survives across frames; a drag that
                        // started in this very frame is used for the first time.
                        drag_from = self.playlist_drag_from.or(drag_started);
                        if let (Some(from), Some(pointer)) =
                            (drag_from, ui.ctx().pointer_latest_pos())
                        {
                            let clip = ui.clip_rect();
                            if clip.contains(pointer) && !row_rects.is_empty() {
                                // Insertion slot: the visible row whose centre is below
                                // the pointer, or after the last visible row.
                                let last_visible_idx = visible_rows.last().map_or(total_rows, |&i| i + 1);
                                let mut insert = last_visible_idx;
                                for (idx, rect) in &row_rects {
                                    if pointer.y < rect.center().y {
                                        insert = *idx;
                                        break;
                                    }
                                }
                                drop_insert = Some(insert);
                                // The source row may be scrolled out of view;
                                // keep the grab cursor for the whole drag.
                                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);

                                if insert != from && insert != from + 1 {
                                    let gap = ui.spacing().item_spacing.y;
                                    let line_y = if let Some((_, rect)) =
                                        row_rects.iter().find(|(i, _)| *i == insert)
                                    {
                                        Some(rect.top() - gap * 0.5)
                                    } else {
                                        row_rects.last().map(|(_, rect)| rect.bottom() + gap * 0.5)
                                    };
                                    if let (Some(y), Some((_, first))) =
                                        (line_y, row_rects.first())
                                    {
                                        ui.painter().line_segment(
                                            [
                                                pos2(first.left() + 6.0, y),
                                                pos2(first.right() - 6.0, y),
                                            ],
                                            Stroke::new(2.0_f32, t.accent),
                                        );
                                    }
                                }

                                // Auto-scroll near the viewport edges so a row
                                // can be dropped beyond the visible tracks.
                                let edge = 24.0;
                                if pointer.y < clip.top() + edge {
                                    ui.scroll_with_delta(vec2(0.0, 18.0));
                                } else if pointer.y > clip.bottom() - edge {
                                    ui.scroll_with_delta(vec2(0.0, -18.0));
                                }
                            }
                        }

                        // Keep the selected row in view after a button move,
                        // with the smallest scroll that makes it visible.
                        if scroll_to_selected {
                            if let Some(rect) = selected_rect {
                                ui.scroll_to_rect(rect, None);
                            } else if let Some(visible_pos) =
                                visible_rows.iter().position(|&idx| idx == selected_idx)
                            {
                                let stride = 48.0 + ui.spacing().item_spacing.y;
                                let top = ui.min_rect().top() + visible_pos as f32 * stride;
                                let target = Rect::from_min_size(
                                    pos2(ui.min_rect().left(), top),
                                    vec2(ui.available_width(), 48.0),
                                );
                                ui.scroll_to_rect(target, None);
                            }
                        }
                    });
                if scroll_to_selected {
                    self.playlist_scroll_to_selected = false;
                }

                if ui.input(|i| i.pointer.primary_released()) {
                    if let Some(from) = drag_from {
                        self.selected_playlist_idx = from;
                        self.sync_playlist_search_cursor();
                        if let Some(insert) = drop_insert {
                            let to = if insert > from { insert - 1 } else { insert };
                            if to != from && to < self.playlist.len() {
                                self.move_selected_playlist_entry(to);
                            }
                        }
                    }
                    self.playlist_drag_from = None;
                } else if let Some(from) = drag_started {
                    self.selected_playlist_idx = from;
                    self.playlist_drag_from = Some(from);
                }
                if drag_from.is_none()
                    && let Some(idx) = select
                {
                    self.selected_playlist_idx = idx;
                    self.sync_playlist_search_cursor();
                }
                if drag_from.is_none()
                    && let Some(idx) = play
                {
                    self.play_playlist_index(idx);
                    self.sync_playlist_search_cursor();
                    // Double-click plays in place; the user stays in the
                    // playlist to keep browsing or navigating search matches.
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
                let title = match self.hvsc_browser.release {
                    Some(release) => format!("High Voltage SID Collection #{release}"),
                    None => "High Voltage SID Collection".to_string(),
                };
                ui.label(
                    RichText::new(title)
                        .font(theme::semibold(22.0))
                        .color(t.text),
                );
                ui.label(
                    RichText::new("Browse the mirror or search titles, authors and paths.")
                        .font(theme::regular(12.0))
                        .color(t.text_muted),
                );
                ui.label(
                    RichText::new(format!("Mirror: {}", self.config.active_hvsc_mirror().name))
                        .font(theme::regular(11.5))
                        .color(t.text_faint),
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
                        let count = self.hvsc_search_results.len();
                        let summary = if count >= 100 {
                            "first 100 results · double-click to play".to_string()
                        } else {
                            format!("{count} results · double-click to play")
                        };
                        ui.label(
                            RichText::new(summary)
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                        );
                    });
                    ui.add_space(4.0);
                    let mut play_path = None;
                    let total_results = self.hvsc_search_results.len();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, 38.0, total_results, |ui, row_range| {
                            for i in row_range {
                                let path = &self.hvsc_search_results[i];
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
                let total_entries = self.hvsc_browser.entries.len();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, 48.0, total_entries, |ui, row_range| {
                        for i in row_range {
                            let entry = &self.hvsc_browser.entries[i];
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

    fn random_list_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
                            RichText::new("Random List")
                                .font(theme::semibold(22.0))
                                .color(t.text),
                        );
                        ui.label(
                            RichText::new(
                                "Create a random playlist from the HVSC tune index. Choose 5–100 tracks, \
                                 pick one or more collections and give the list a name. The .m3u is \
                                 saved to your playlist library and becomes active.",
                            )
                            .font(theme::regular(12.0))
                            .color(t.text_muted),
                        );
                        ui.add_space(16.0);

                        settings_card(ui, &t, "Playlist settings", |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Playlist name")
                                        .font(theme::medium(12.0))
                                        .color(t.text),
                                );
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.random_playlist_name)
                                        .hint_text("e.g. Random C64 Gems")
                                        .desired_width(280.0)
                                        .font(theme::regular(13.0)),
                                );
                                if resp.changed() {
                                    self.random_list_error = None;
                                }
                            });

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("SID tracks")
                                        .font(theme::medium(12.0))
                                        .color(t.text),
                                );
                                let previous = self.random_track_count;
                                ui.add(
                                    egui::DragValue::new(&mut self.random_track_count)
                                        .range(5..=100)
                                        .speed(1.0),
                                );
                                ui.label(
                                    RichText::new("5–100")
                                        .font(theme::regular(11.5))
                                        .color(t.text_faint),
                                );
                                if previous != self.random_track_count {
                                    self.random_list_error = None;
                                }
                            });

                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("Collections")
                                    .font(theme::medium(12.0))
                                    .color(t.text),
                            );
                            ui.horizontal(|ui| {
                                let changed =
                                    ui.checkbox(&mut self.random_collections.demos, "Demos").changed();
                                if changed {
                                    self.random_list_error = None;
                                }
                                let changed =
                                    ui.checkbox(&mut self.random_collections.games, "Games").changed();
                                if changed {
                                    self.random_list_error = None;
                                }
                                let changed = ui
                                    .checkbox(&mut self.random_collections.musicians, "Musicians")
                                    .changed();
                                if changed {
                                    self.random_list_error = None;
                                }
                            });

                            ui.add_space(8.0);
                            let available =
                                self.hvsc_browser.collection_count(self.random_collections);
                            ui.label(
                                RichText::new(format!(
                                    "{available} matching SID files in the loaded HVSC index."
                                ))
                                .font(theme::regular(11.5))
                                .color(t.text_muted),
                            );

                            if let Some(err) = &self.random_list_error {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(err)
                                        .font(theme::regular(11.5))
                                        .color(t.accent_text),
                                );
                            }

                            ui.add_space(12.0);
                            if primary_button(ui, &t, "Create random playlist").clicked() {
                                self.create_random_playlist();
                            }
                        });
                    });
            });
    }

    fn export_view(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());

        // Check if an export just finished
        let export_done = self
            .export_progress
            .as_ref()
            .map_or(false, |p| p.is_done());
        if export_done {
            if let Some(p) = self.export_progress.take() {
                if let Some(err) = p.error_message() {
                    self.set_status(format!("Export failed: {err}"));
                } else if !p.is_cancelled() {
                    if let Some(ref path) = self.export_last_path {
                        self.set_status(format!("Export saved: {}", path.display()));
                    } else {
                        self.set_status("Export complete");
                    }
                } else {
                    self.set_status("Export cancelled");
                }
            }
        }

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
                            RichText::new("Export")
                                .font(theme::semibold(22.0))
                                .color(t.text),
                        );
                        let subtitle = if self.sid_file.name.is_empty() {
                            "No active track. Open a SID file to enable export.".to_string()
                        } else {
                            format!("Export \"{}\" to WAV, FLAC or MP3.", self.track_title())
                        };
                        ui.label(
                            RichText::new(subtitle)
                                .font(theme::regular(12.0))
                                .color(t.text_muted),
                        );
                        ui.add_space(16.0);

                        // ── Active Track Details ────────────────────────────
                        settings_card(ui, &t, "Active SID Track", |ui| {
                            let title = self.track_title();
                            property_row(ui, &t, "Title", &title);
                            let author = if self.sid_file.author.is_empty() {
                                "Unknown author".to_string()
                            } else {
                                self.sid_file.author.clone()
                            };
                            property_row(ui, &t, "Author", &author);
                            if !self.sid_file.released.is_empty() {
                                property_row(ui, &t, "Released", &self.sid_file.released);
                            }
                            property_row(ui, &t, "Clock", self.clock_label());
                            property_row(ui, &t, "SID chip", self.chip_label());
                            property_row(ui, &t, "Chips", self.sid_count_label());
                            property_row(
                                ui,
                                &t,
                                "Subsong",
                                &format!("{} of {}", self.export_settings.subsong, self.total_songs),
                            );

                            if self.total_songs > 1 {
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Select Subsong")
                                            .font(theme::medium(12.0))
                                            .color(t.text),
                                    );
                                    ui.add_space(8.0);
                                    if icon_button(ui, &t, "chevron-left", 26.0, false, "Previous subsong").clicked()
                                        && self.export_settings.subsong > 1
                                    {
                                        self.export_settings.subsong -= 1;
                                        if let Some(dur) = self.hvsc_browser.song_duration(&self.sid_file.md5, self.export_settings.subsong) {
                                            self.export_settings.duration_secs = dur.as_secs_f32();
                                        }
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "{:02} / {:02}",
                                            self.export_settings.subsong, self.total_songs
                                        ))
                                        .font(theme::mono(12.0))
                                        .color(t.text_muted),
                                    );
                                    if icon_button(ui, &t, "chevron-right", 26.0, false, "Next subsong").clicked()
                                        && self.export_settings.subsong < self.total_songs
                                    {
                                        self.export_settings.subsong += 1;
                                        if let Some(dur) = self.hvsc_browser.song_duration(&self.sid_file.md5, self.export_settings.subsong) {
                                            self.export_settings.duration_secs = dur.as_secs_f32();
                                        }
                                    }
                                });
                            }
                        });

                        // ── SID Fine Tuning ─────────────────────────────────
                        settings_card(ui, &t, "SID Fine Tuning", |ui| {
                            // EKV filter
                            let mut ekv = self.export_settings.ekv_filter;
                            if ui.checkbox(&mut ekv, "EKV transistor filter (6581 model)").changed() {
                                self.export_settings.ekv_filter = ekv;
                            }
                            ui.label(
                                RichText::new("Enables non-linear transistor ladder simulation for MOS 6581.")
                                    .font(theme::regular(11.0))
                                    .color(t.text_faint),
                            );

                            ui.add_space(10.0);

                            // Voice mutes
                            ui.label(
                                RichText::new("Voice Configuration")
                                    .font(theme::medium(12.5))
                                    .color(t.text),
                            );
                            ui.label(
                                RichText::new("Select voices to mute during export (unmute = active in mix):")
                                    .font(theme::regular(11.0))
                                    .color(t.text_faint),
                            );
                            ui.add_space(4.0);

                            let voice_count = self.total_voices();
                            if self.export_settings.voice_mutes.len() < voice_count {
                                self.export_settings.voice_mutes.resize(voice_count, false);
                            }

                            ui.horizontal_wrapped(|ui| {
                                for i in 0..voice_count {
                                    let is_muted = self.export_settings.voice_mutes.get(i).copied().unwrap_or(false);
                                    let mut active = !is_muted;
                                    let label = format!("Voice {}", i + 1);
                                    if ui.checkbox(&mut active, &label).changed() {
                                        if let Some(slot) = self.export_settings.voice_mutes.get_mut(i) {
                                            *slot = !active;
                                        }
                                    }
                                }
                            });
                        });

                        // ── Format & Audio Settings ─────────────────────────
                        settings_card(ui, &t, "Format & Quality", |ui| {
                            ui.label(
                                RichText::new("Export Format")
                                    .font(theme::medium(12.5))
                                    .color(t.text),
                            );
                            let fmt_labels = ["WAV", "FLAC", "MP3"];
                            let fmt_idx = match self.export_settings.format {
                                ExportFormat::Wav => 0,
                                ExportFormat::Flac => 1,
                                ExportFormat::Mp3 => 2,
                            };
                            if let Some(choice) = segmented(ui, &t, fmt_idx, &fmt_labels) {
                                self.export_settings.format = match choice {
                                    1 => ExportFormat::Flac,
                                    2 => ExportFormat::Mp3,
                                    _ => ExportFormat::Wav,
                                };
                            }
                            ui.label(
                                RichText::new(match self.export_settings.format {
                                    ExportFormat::Wav => "Lossless 16-bit uncompressed PCM (native support, universal compatibility).",
                                    ExportFormat::Flac => "Lossless compressed audio (via ffmpeg if installed, or high-definition WAV).",
                                    ExportFormat::Mp3 => "MPEG Layer-3 compressed audio (delegates to ffmpeg libmp3lame).",
                                })
                                .font(theme::regular(11.0))
                                .color(t.text_faint),
                            );

                            ui.add_space(10.0);

                            // Duration
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [110.0, 18.0],
                                    egui::Label::new(
                                        RichText::new("Duration")
                                            .font(theme::regular(11.5))
                                            .color(t.text_faint),
                                    ),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut self.export_settings.duration_secs)
                                        .speed(1.0)
                                        .range(1.0..=1200.0)
                                        .suffix(" s"),
                                );
                                let secs = self.export_settings.duration_secs as u64;
                                ui.label(
                                    RichText::new(format!("({:02}:{:02})", secs / 60, secs % 60))
                                        .font(theme::mono(11.0))
                                        .color(t.text_muted),
                                );

                                if let Some(d) = self.hvsc_browser.song_duration(&self.sid_file.md5, self.export_settings.subsong) {
                                    let hsecs = d.as_secs();
                                    if secondary_button(
                                        ui,
                                        &t,
                                        &format!("HVSC Length: {:02}:{:02}", hsecs / 60, hsecs % 60),
                                    )
                                    .on_hover_text("Load tune duration from HVSC database")
                                    .clicked()
                                    {
                                        self.export_settings.duration_secs = d.as_secs_f32();
                                    }
                                }
                            });

                            ui.add_space(6.0);

                            // Fade-out
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [110.0, 18.0],
                                    egui::Label::new(
                                        RichText::new("Fade-out")
                                            .font(theme::regular(11.5))
                                            .color(t.text_faint),
                                    ),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut self.export_settings.fade_out_secs)
                                        .speed(0.5)
                                        .range(0.0..=30.0)
                                        .suffix(" s"),
                                );
                            });

                            ui.add_space(6.0);

                            // Sample rate
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [110.0, 18.0],
                                    egui::Label::new(
                                        RichText::new("Sample rate")
                                            .font(theme::regular(11.5))
                                            .color(t.text_faint),
                                    ),
                                );
                                egui::ComboBox::from_id_salt("export-rate")
                                    .width(120.0)
                                    .selected_text(format!("{} Hz", self.export_settings.sample_rate))
                                    .show_ui(ui, |ui| {
                                        for &rate in &[22050u32, 44100, 48000, 96000] {
                                            ui.selectable_value(
                                                &mut self.export_settings.sample_rate,
                                                rate,
                                                format!("{rate} Hz"),
                                            );
                                        }
                                    });
                            });

                            ui.add_space(6.0);

                            // Master gain
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [110.0, 18.0],
                                    egui::Label::new(
                                        RichText::new("Master gain")
                                            .font(theme::regular(11.5))
                                            .color(t.text_faint),
                                    ),
                                );
                                ui.add(
                                    egui::Slider::new(&mut self.export_settings.gain, 0.0..=1.5)
                                        .show_value(true),
                                );
                            });

                            // MP3 bitrate
                            if self.export_settings.format == ExportFormat::Mp3 {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [110.0, 18.0],
                                        egui::Label::new(
                                            RichText::new("MP3 bitrate")
                                                .font(theme::regular(11.5))
                                                .color(t.text_faint),
                                        ),
                                    );
                                    egui::ComboBox::from_id_salt("export-mp3-kbps")
                                        .width(120.0)
                                        .selected_text(format!("{} kbps", self.export_settings.mp3_bitrate_kbps))
                                        .show_ui(ui, |ui| {
                                            for &br in &[128u32, 192, 256, 320] {
                                                ui.selectable_value(
                                                    &mut self.export_settings.mp3_bitrate_kbps,
                                                    br,
                                                    format!("{br} kbps"),
                                                );
                                            }
                                        });
                                });
                            }
                        });

                        // ── Destination Directory ───────────────────────────
                        settings_card(ui, &t, "Save Location", |ui| {
                            ui.horizontal(|ui| {
                                let dir_display = match &self.export_output_dir {
                                    Some(d) => d.to_string_lossy().to_string(),
                                    None => "Default folder (choose destination on export)".to_string(),
                                };
                                ui.label(
                                    RichText::new(dir_display)
                                        .font(theme::mono(11.5))
                                        .color(if self.export_output_dir.is_some() {
                                            t.text
                                        } else {
                                            t.text_faint
                                        }),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if secondary_button(ui, &t, "Choose folder…").clicked() {
                                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                            self.export_output_dir = Some(folder);
                                        }
                                    }
                                });
                            });

                            if let Some(ref path) = self.export_last_path {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(format!("Last exported: {}", path.display()))
                                        .font(theme::regular(11.0))
                                        .color(t.text_muted),
                                );
                            }
                        });

                        // ── Export Render / Progress ────────────────────────
                        let has_track = !self.sid_file.name.is_empty();

                        if let Some(ref progress) = self.export_progress {
                            let frac = progress.fraction();
                            settings_card(ui, &t, "Exporting Audio…", |ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 12.0),
                                    Sense::hover(),
                                );
                                let track = Rect::from_center_size(
                                    rect.center(),
                                    vec2(rect.width(), 6.0),
                                );
                                ui.painter().rect_filled(track, CornerRadius::same(3), t.hover);
                                let fill = Rect::from_min_size(
                                    track.min,
                                    vec2(track.width() * frac, track.height()),
                                );
                                ui.painter().rect_filled(fill, CornerRadius::same(3), t.accent);

                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{:.1}% rendered", frac * 100.0))
                                            .font(theme::mono(12.0))
                                            .color(t.text),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if secondary_button(ui, &t, "Cancel").clicked() {
                                            progress.cancel.store(true, Ordering::Relaxed);
                                        }
                                    });
                                });
                            });
                        } else {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if has_track {
                                    let export_label = format!("Export to {}…", self.export_settings.format.label());
                                    if primary_button(ui, &t, &export_label).clicked() {
                                        self.start_export();
                                    }
                                } else {
                                    let font = theme::medium(12.5);
                                    let label = "Export…";
                                    let w = text_width(ui, label, &font) + 26.0;
                                    let (rect, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                                    ui.painter().rect_filled(rect, CornerRadius::same(7), t.hover);
                                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, t.text_faint);
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new("Open a SID file first from File or Home.")
                                            .font(theme::regular(12.0))
                                            .color(t.text_faint),
                                    );
                                }
                            });
                        }
                    });
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
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new("Accent colour")
                                    .font(theme::medium(12.5))
                                    .color(t.text),
                            );
                            if let Some(accent) = accent_picker(ui, &t, self.accent) {
                                self.accent = accent;
                            }
                            ui.label(
                                RichText::new("Changes buttons, selected rows, tabs and other active controls.")
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

                        #[cfg(target_os = "linux")]
                        settings_card(ui, &t, "Window", |ui| {
                            if crate::x11_display_available() {
                                let mut remember_positions = !self.config.prefer_wayland;
                                if ui
                                    .checkbox(&mut remember_positions, "Remember window positions")
                                    .changed()
                                {
                                    self.config.prefer_wayland = !remember_positions;
                                    self.config.save();
                                }
                                ui.label(
                                    RichText::new(
                                        "SidCraft uses X11 (XWayland works too) so the full \
                                         window and the mini player each reopen where you left \
                                         them. Native Wayland does not let applications read or \
                                         set window positions; clear this only if you prefer \
                                         Wayland and do not need position memory. Takes effect \
                                         after restarting SidCraft.",
                                    )
                                    .font(theme::regular(11.5))
                                    .color(t.text_faint),
                                );
                            } else {
                                ui.label(
                                    RichText::new(
                                        "No X11/XWayland display was found, so the compositor \
                                         places windows and SidCraft cannot remember their \
                                         positions. Enable XWayland to restore separate \
                                         positions for the full window and the mini player.",
                                    )
                                    .font(theme::regular(11.5))
                                    .color(t.text_faint),
                                );
                            }
                        });

                        settings_card(ui, &t, "Mini player", |ui| {
                            ui.label(
                                RichText::new("Opacity")
                                    .font(theme::medium(12.5))
                                    .color(t.text),
                            );
                            let mut opacity = self.mini_opacity;
                            ui.add(
                                egui::Slider::new(&mut opacity, 0.25..=1.0)
                                    .text("")
                                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                            );
                            if (opacity - self.mini_opacity).abs() > 0.001 {
                                self.mini_opacity = opacity;
                            }
                            ui.label(
                                RichText::new(
                                    "Transparency is drawn by SidCraft and works on Windows, \
                                     macOS and Linux desktops.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_faint),
                            );
                            ui.add_space(8.0);
                            let mut start = self.start_in_mini;
                            if ui.checkbox(&mut start, "Start in mini player").changed() {
                                self.start_in_mini = start;
                            }
                            ui.label(
                                RichText::new(
                                    "Opens the frameless player on launch. Return to the full \
                                     shell with the expand button or Ctrl+M.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_faint),
                            );
                        });

                        settings_card(ui, &t, "Playlists", |ui| {
                            let mut show = self.config.show_favourite_top_100;
                            if ui.checkbox(&mut show, "Favourite Top 100 SIDs").changed() {
                                self.config.show_favourite_top_100 = show;
                                self.sync_favourite_playlist();
                            }
                            ui.label(
                                RichText::new(
                                    "Shows the Favourite Top 100 SIDs playlist in the playlist \
                                     library. Turning it off removes its M3U file.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_faint),
                            );
                        });

                        settings_card(ui, &t, "HVSC mirrors", |ui| {
                            ui.label(
                                RichText::new(
                                    "The selected mirror is used for browsing, search, random playlists and HVSC playlist entries.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_faint),
                            );
                            ui.add_space(8.0);

                            let mirrors = self.config.hvsc_mirrors.clone();
                            let active_id = self.config.active_hvsc_mirror;
                            let selected_name = mirrors
                                .iter()
                                .find(|mirror| mirror.id == active_id)
                                .map(|mirror| mirror.name.clone())
                                .unwrap_or_default();
                            let mut selected = active_id;
                            egui::ComboBox::from_id_salt("settings-hvsc-mirror")
                                .width(220.0)
                                .selected_text(&selected_name)
                                .show_ui(ui, |ui| {
                                    for mirror in &mirrors {
                                        ui.selectable_value(
                                            &mut selected,
                                            mirror.id,
                                            mirror.name.as_str(),
                                        );
                                    }
                                });
                            if selected != active_id {
                                self.select_hvsc_mirror(selected);
                            }
                            ui.add_space(10.0);

                            for mirror in &mirrors {
                                ui.horizontal(|ui| {
                                    let name = if mirror.id == active_id {
                                        format!("{}  (default)", mirror.name)
                                    } else {
                                        mirror.name.clone()
                                    };
                                    ui.label(
                                        RichText::new(name)
                                            .font(theme::medium(12.0))
                                            .color(t.text),
                                    );
                                    ui.label(
                                        RichText::new(&mirror.location)
                                            .font(theme::regular(11.0))
                                            .color(t.text_faint),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    if mirror.id != active_id
                                        && secondary_button(ui, &t, "Make default").clicked()
                                    {
                                        self.select_hvsc_mirror(mirror.id);
                                    }
                                    if secondary_button(ui, &t, "Edit").clicked() {
                                        self.open_hvsc_mirror_editor(Some(mirror.id));
                                    }
                                    if secondary_button(ui, &t, "Remove").clicked() {
                                        self.remove_hvsc_mirror(mirror.id);
                                    }
                                });
                                ui.add_space(6.0);
                            }

                            ui.horizontal(|ui| {
                                if secondary_button(ui, &t, "Add mirror").clicked() {
                                    self.open_hvsc_mirror_editor(None);
                                }
                                if secondary_button(ui, &t, "Add local folder…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                                {
                                    self.add_local_hvsc_folder(path);
                                }
                            });
                            ui.label(
                                RichText::new(
                                    "A local folder must point at the C64Music directory of an HVSC copy.",
                                )
                                .font(theme::regular(11.0))
                                .color(t.text_faint),
                            );
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

                        settings_card(ui, &t, "Updates", |ui| {
                            let mut check = self.config.check_updates;
                            if ui
                                .checkbox(&mut check, "Check for new versions at startup")
                                .changed()
                            {
                                self.config.check_updates = check;
                                self.config.save();
                                if check {
                                    self.start_update_check(true);
                                }
                            }
                            ui.label(
                                RichText::new(
                                    "SidCraft asks GitHub for the newest stable release about 30 \
                                     seconds after it opens. Pre-releases are ignored.",
                                )
                                .font(theme::regular(11.5))
                                .color(t.text_faint),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                let busy = self.update_busy();
                                let clicked = ui
                                    .add_enabled_ui(!busy, |ui| {
                                        secondary_button(ui, &t, "Check now")
                                    })
                                    .inner
                                    .clicked();
                                if clicked {
                                    self.start_update_check(true);
                                }
                                if let Some(line) = self.update_about_line() {
                                    ui.label(
                                        RichText::new(line)
                                            .font(theme::regular(11.5))
                                            .color(t.text_muted),
                                    );
                                }
                            });
                        });
                    });
            });
    }

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


