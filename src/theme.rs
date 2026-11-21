// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from PdfCraft (https://github.com/storytold/pdfcraft),
// crate `ui-egui`, file `src/theme.rs`.
// Copyright (c) 2026 ArtCraft Team and the PdfCraft contributors.
// PdfCraft is licensed MIT OR Apache-2.0; the MIT option is elected here so the
// derived design tokens can be combined into this GPL-3.0-only work.
// See licenses/LICENSE-PDFCRAFT-MIT.txt and licenses/LICENSE-PDFCRAFT-APACHE-2.0.txt.
//! Design tokens and egui styling for the SidCraft shell.
//!
//! The palette and layout measurements deliberately follow the PdfCraft / Craft
//! desktop grammar: neutral chrome, white (or charcoal) panels, one blue accent,
//! rounded cards and a quiet, text-first hierarchy. Every custom widget reads
//! [`Tokens::get`] rather than hard-coding colours.

use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Stroke, Visuals};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeKind {
    Light,
    #[default]
    Dark,
}

/// The saved user choice, independent of the light/dark colours currently shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    /// The colours to draw. `system` is what the operating system says, `None`
    /// when it says nothing; `fallback` is used for an unknown desktop.
    pub fn resolve(self, system: Option<bool>, fallback: ThemeKind) -> ThemeKind {
        match self {
            Self::Light => ThemeKind::Light,
            Self::Dark => ThemeKind::Dark,
            Self::System => match system {
                Some(true) => ThemeKind::Dark,
                Some(false) => ThemeKind::Light,
                None => fallback,
            },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Scale a colour's alpha by `alpha` (0..=255), preserving its RGB channels.
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        ((color.a() as u16 * alpha as u16) / 255) as u8,
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub kind: ThemeKind,
    /// Title / tab strip.
    pub titlebar: Color32,
    /// Mode bar and rails.
    pub chrome: Color32,
    pub panel: Color32,
    /// Document area behind cards and pages.
    pub pasteboard: Color32,
    pub card: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selected: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub field: Color32,
    pub page_shadow: Color32,
    pub radius: u8,
}

impl Tokens {
    pub fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => Self {
                kind,
                titlebar: Color32::from_rgb(0xE9, 0xE9, 0xEB),
                chrome: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                pasteboard: Color32::from_rgb(0xF1, 0xF1, 0xF3),
                card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                border: Color32::from_rgb(0xDA, 0xDA, 0xDE),
                divider: Color32::from_rgb(0xE8, 0xE8, 0xEB),
                text: Color32::from_rgb(0x22, 0x22, 0x26),
                text_muted: Color32::from_rgb(0x5E, 0x5E, 0x66),
                text_faint: Color32::from_rgb(0x6B, 0x6B, 0x73),
                icon: Color32::from_rgb(0x44, 0x44, 0x4B),
                hover: Color32::from_rgb(0xF0, 0xF0, 0xF3),
                pressed: Color32::from_rgb(0xE4, 0xE4, 0xE9),
                selected: Color32::from_rgb(0xE6, 0xEE, 0xFD),
                accent: Color32::from_rgb(0x1B, 0x63, 0xE0),
                accent_text: Color32::from_rgb(0x17, 0x55, 0xC4),
                accent_soft: Color32::from_rgb(0xE3, 0xEC, 0xFD),
                field: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                page_shadow: Color32::from_black_alpha(34),
                radius: 6,
            },
            ThemeKind::Dark => Self {
                kind,
                titlebar: Color32::from_rgb(0x1B, 0x1B, 0x1E),
                chrome: Color32::from_rgb(0x26, 0x26, 0x2A),
                panel: Color32::from_rgb(0x26, 0x26, 0x2A),
                pasteboard: Color32::from_rgb(0x19, 0x19, 0x1C),
                card: Color32::from_rgb(0x2E, 0x2E, 0x33),
                border: Color32::from_rgb(0x3C, 0x3C, 0x43),
                divider: Color32::from_rgb(0x33, 0x33, 0x39),
                text: Color32::from_rgb(0xEC, 0xEC, 0xEF),
                text_muted: Color32::from_rgb(0xAE, 0xAE, 0xB6),
                text_faint: Color32::from_rgb(0x97, 0x97, 0x9E),
                icon: Color32::from_rgb(0xD4, 0xD4, 0xDA),
                hover: Color32::from_rgb(0x34, 0x34, 0x3A),
                pressed: Color32::from_rgb(0x3E, 0x3E, 0x45),
                selected: Color32::from_rgb(0x23, 0x3A, 0x63),
                accent: Color32::from_rgb(0x4B, 0x8B, 0xF5),
                accent_text: Color32::from_rgb(0x8C, 0xB6, 0xFA),
                accent_soft: Color32::from_rgb(0x24, 0x36, 0x57),
                field: Color32::from_rgb(0x1E, 0x1E, 0x22),
                page_shadow: Color32::from_black_alpha(120),
                radius: 6,
            },
        }
    }

    /// Store the tokens for widgets drawn later in this frame.
    pub fn store(ctx: &egui::Context, tokens: Self) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("sidcraft-theme"), tokens));
    }

    /// The tokens for the current frame, falling back to dark before the first apply.
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new("sidcraft-theme")))
            .unwrap_or_else(|| Self::for_kind(ThemeKind::Dark))
    }

    /// Scale every token alpha by `alpha` (0..=255), keeping the same palette.
    /// Used by the frameless mini player to apply its opacity setting without
    /// changing the tokens stored for the rest of the shell.
    pub fn with_opacity(mut self, alpha: u8) -> Self {
        self.titlebar = with_alpha(self.titlebar, alpha);
        self.chrome = with_alpha(self.chrome, alpha);
        self.panel = with_alpha(self.panel, alpha);
        self.pasteboard = with_alpha(self.pasteboard, alpha);
        self.card = with_alpha(self.card, alpha);
        self.border = with_alpha(self.border, alpha);
        self.divider = with_alpha(self.divider, alpha);
        self.text = with_alpha(self.text, alpha);
        self.text_muted = with_alpha(self.text_muted, alpha);
        self.text_faint = with_alpha(self.text_faint, alpha);
        self.icon = with_alpha(self.icon, alpha);
        self.hover = with_alpha(self.hover, alpha);
        self.pressed = with_alpha(self.pressed, alpha);
        self.selected = with_alpha(self.selected, alpha);
        self.accent = with_alpha(self.accent, alpha);
        self.accent_text = with_alpha(self.accent_text, alpha);
        self.accent_soft = with_alpha(self.accent_soft, alpha);
        self.field = with_alpha(self.field, alpha);
        self.page_shadow = with_alpha(self.page_shadow, alpha);
        self
    }

    pub fn dark(&self) -> bool {
        self.kind == ThemeKind::Dark
    }

    pub fn radius(&self) -> CornerRadius {
        CornerRadius::same(self.radius)
    }

    /// A voice trace colour. The first voice is the interface accent so the
    /// visualiser belongs to the same palette as the rest of the shell.
    pub fn voice(&self, index: usize) -> Color32 {
        const VOICES: [Color32; 6] = [
            Color32::from_rgb(0x54, 0x9A, 0xFF),
            Color32::from_rgb(0x2F, 0xC5, 0xB0),
            Color32::from_rgb(0xF2, 0xB3, 0x3C),
            Color32::from_rgb(0xF2, 0x6D, 0xA6),
            Color32::from_rgb(0xA0, 0x7B, 0xF5),
            Color32::from_rgb(0x6B, 0xD6, 0x7C),
        ];
        if index == 0 {
            self.accent
        } else {
            // `accent.a()` carries the mini player's opacity after
            // `with_opacity`, so the fixed voice colours follow it too.
            with_alpha(VOICES[index % VOICES.len()], self.accent.a())
        }
    }
}

/// Install the tokens and a complete egui style for `kind`.
pub fn apply(ctx: &egui::Context, kind: ThemeKind) {
    let t = Tokens::for_kind(kind);
    Tokens::store(ctx, t);

    let mut visuals = if t.dark() { Visuals::dark() } else { Visuals::light() };
    visuals.panel_fill = t.pasteboard;
    visuals.window_fill = t.panel;
    visuals.faint_bg_color = t.chrome;
    visuals.extreme_bg_color = t.field;
    visuals.window_stroke = Stroke::new(1.0_f32, t.border);
    visuals.window_corner_radius = t.radius();
    visuals.menu_corner_radius = t.radius();
    visuals.selection.bg_fill = t.selected;
    visuals.selection.stroke = Stroke::new(1.0_f32, t.accent);
    visuals.hyperlink_color = t.accent_text;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = t.card;
    widgets.noninteractive.weak_bg_fill = t.panel;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.divider);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text);
    widgets.noninteractive.corner_radius = t.radius();
    widgets.inactive.bg_fill = t.card;
    widgets.inactive.weak_bg_fill = t.card;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, t.border);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, t.text);
    widgets.inactive.corner_radius = t.radius();
    widgets.hovered.bg_fill = t.hover;
    widgets.hovered.weak_bg_fill = t.hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, t.border);
    widgets.hovered.fg_stroke = Stroke::new(1.0_f32, t.text);
    widgets.hovered.corner_radius = t.radius();
    widgets.active.bg_fill = t.pressed;
    widgets.active.weak_bg_fill = t.pressed;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, t.accent);
    widgets.active.fg_stroke = Stroke::new(1.0_f32, t.text);
    widgets.active.corner_radius = t.radius();
    widgets.open.bg_fill = t.hover;
    widgets.open.weak_bg_fill = t.hover;
    widgets.open.bg_stroke = Stroke::new(1.0_f32, t.border);
    widgets.open.fg_stroke = Stroke::new(1.0_f32, t.text);
    widgets.open.corner_radius = t.radius();

    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size = egui::vec2(28.0, 26.0);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.visuals.window_shadow = egui::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if t.dark() { 110 } else { 40 }),
        };
        style.visuals.popup_shadow = style.visuals.window_shadow;
        style.text_styles.insert(
            egui::TextStyle::Body,
            FontId::proportional(13.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            FontId::proportional(13.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            FontId::proportional(11.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::proportional(18.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            FontId::monospace(12.0),
        );
    });
}

// Typography helpers: the shell uses one proportional family and one mono family.

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn semibold(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Pointing hand, the cursor for a control that clicks.
pub fn hand(resp: egui::Response) -> egui::Response {
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
