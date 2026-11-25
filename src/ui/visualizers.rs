// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

use super::*;
impl SidcraftApp {
    pub(super) fn display_chrome(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) -> Rect {
        let painter = ui.painter();
        if compact {
            // The mini player only needs the visualiser itself: the large
            // clock, ticker and status strip of the full display would swamp
            // a small panel, so draw a plain framed drawing area instead.
            painter.rect_filled(
                rect,
                CornerRadius::same(6),
                theme::with_alpha(Color32::from_rgb(0x0F, 0x10, 0x14), t.accent.a()),
            );
            painter.rect_stroke(
                rect,
                CornerRadius::same(6),
                Stroke::new(1.0_f32, Color32::from_white_alpha(22)),
                StrokeKind::Inside,
            );
            return rect.shrink(6.0);
        }
        painter.rect_filled(
            rect,
            CornerRadius::same(10),
            theme::with_alpha(Color32::from_rgb(0x0F, 0x10, 0x14), t.accent.a()),
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

    pub(super) fn paint_scope(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

    pub(super) fn paint_display_vu(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

    pub(super) fn paint_spectrum(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

    pub(super) fn paint_spectrogram(&mut self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

        let bands = self.spectrogram.columns.front().map_or(1, Vec::len).max(1);
        let step = (bands / 48).max(1);
        let rows = bands.div_ceil(step);

        if self.spectrogram_dirty || self.spectrogram_texture.is_none() {
            let mut pixels = vec![Color32::TRANSPARENT; SPECTROGRAM_COLUMNS * rows];
            let x_offset = SPECTROGRAM_COLUMNS.saturating_sub(columns);
            for (c, column) in self.spectrogram.columns.iter().enumerate() {
                let x = x_offset + c;
                if x >= SPECTROGRAM_COLUMNS {
                    break;
                }
                for row in 0..rows {
                    let lo = row * step;
                    let hi = (lo + step).min(column.len());
                    if lo >= hi {
                        break;
                    }
                    let value = column[lo..hi].iter().copied().fold(0.0_f32, f32::max);
                    // Image row 0 is at the top (highest frequency).
                    let y = rows - 1 - row;
                    pixels[y * SPECTROGRAM_COLUMNS + x] = heat_color(value);
                }
            }
            let image = egui::ColorImage {
                size: [SPECTROGRAM_COLUMNS, rows],
                pixels,
            };
            match &mut self.spectrogram_texture {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.spectrogram_texture = Some(ui.ctx().load_texture(
                        "sidcraft_spectrogram",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            self.spectrogram_dirty = false;
        }

        if let Some(tex) = &self.spectrogram_texture {
            painter.image(
                tex.id(),
                inner,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.line_segment(
            [
                pos2(inner.right() - 0.5, inner.top()),
                pos2(inner.right() - 0.5, inner.bottom()),
            ],
            Stroke::new(1.0_f32, t.accent.gamma_multiply(0.5)),
        );
    }

    pub(super) fn paint_oscilloscope(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

    pub(super) fn paint_piano_roll(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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
                // SID frequency registers can encode notes above the drawn
                // range; clamping keeps the bar inside the black LCD rect.
                let note = sample.note.clamp(MIN_NOTE as u8, MAX_NOTE as u8);
                let mut j = i + 1;
                while j < n && samples[j].gate && samples[j].note == note {
                    j += 1;
                }
                let x0 = inner.left() + inner.width() * i as f32 / n as f32;
                let x1 = (inner.left() + inner.width() * j as f32 / n as f32).max(x0 + 1.5);
                let h = (usable / (MAX_NOTE - MIN_NOTE) * 1.6).max(1.5);
                let y = (top + usable
                    - (note as f32 - MIN_NOTE) / (MAX_NOTE - MIN_NOTE) * usable)
                    .clamp(top + h / 2.0, top + usable - h / 2.0);
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, y - h / 2.0), pos2(x1, y + h / 2.0)),
                    CornerRadius::same(1),
                    color,
                );
                i = j;
            }
        }
    }

    pub(super) fn paint_filter(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

    pub(super) fn paint_adsr(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, compact: bool) {
        let inner = self.display_chrome(ui, t, rect, compact);
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

}

