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

/// The hue used for active controls: buttons, selected rows, tabs, sliders and
/// every other "highlight" colour. Each hue has its own set of tones for light
/// and dark themes, mirroring the original blue palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AccentColor {
    #[default]
    Blue,
    Red,
    Green,
    Yellow,
    Orange,
    Turquoise,
}

impl AccentColor {
    pub const ALL: [Self; 6] = [
        Self::Blue,
        Self::Red,
        Self::Green,
        Self::Yellow,
        Self::Orange,
        Self::Turquoise,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Yellow => "Yellow",
            Self::Orange => "Orange",
            Self::Turquoise => "Turquoise",
        }
    }

    pub fn config_key(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Orange => "orange",
            Self::Turquoise => "turquoise",
        }
    }

    /// Reads a saved accent, falling back to the original blue for unknown
    /// values so older or hand-edited config files stay valid.
    pub fn from_config(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|accent| accent.config_key() == value)
            .unwrap_or_default()
    }

    /// The representative colour shown in the settings swatch.
    pub fn swatch(self) -> Color32 {
        match self {
            Self::Blue => Color32::from_rgb(0x3B, 0x7B, 0xEC),
            Self::Red => Color32::from_rgb(0xDD, 0x4F, 0x4B),
            Self::Green => Color32::from_rgb(0x2A, 0xA6, 0x64),
            Self::Yellow => Color32::from_rgb(0xD9, 0xB3, 0x2E),
            Self::Orange => Color32::from_rgb(0xE6, 0x86, 0x2C),
            Self::Turquoise => Color32::from_rgb(0x22, 0xAC, 0xA8),
        }
    }
}

/// The four related tones derived from a chosen accent hue.
#[derive(Clone, Copy)]
struct AccentTokens {
    accent: Color32,
    accent_text: Color32,
    accent_soft: Color32,
    selected: Color32,
    on_accent: Color32,
}

/// Readable text on top of a filled accent button. Bright accents (notably
/// yellow) use dark text; the original blue keeps its white labels.
fn contrasting_text(background: Color32) -> Color32 {
    fn channel(value: u8) -> f32 {
        let c = f32::from(value) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let luminance = 0.2126 * channel(background.r())
        + 0.7152 * channel(background.g())
        + 0.0722 * channel(background.b());
    if luminance > 0.30 {
        Color32::from_rgb(0x1B, 0x1B, 0x1E)
    } else {
        Color32::WHITE
    }
}

fn accent_tokens(kind: ThemeKind, accent: AccentColor) -> AccentTokens {
    let (base, accent_text, accent_soft, selected) = match (kind, accent) {
        (ThemeKind::Light, AccentColor::Blue) => (
            Color32::from_rgb(0x1B, 0x63, 0xE0),
            Color32::from_rgb(0x17, 0x55, 0xC4),
            Color32::from_rgb(0xE3, 0xEC, 0xFD),
            Color32::from_rgb(0xE6, 0xEE, 0xFD),
        ),
        (ThemeKind::Light, AccentColor::Red) => (
            Color32::from_rgb(0xCE, 0x3B, 0x3B),
            Color32::from_rgb(0xB1, 0x2F, 0x2F),
            Color32::from_rgb(0xFB, 0xE8, 0xE8),
            Color32::from_rgb(0xFA, 0xE5, 0xE5),
        ),
        (ThemeKind::Light, AccentColor::Green) => (
            Color32::from_rgb(0x1F, 0x92, 0x54),
            Color32::from_rgb(0x18, 0x7A, 0x45),
            Color32::from_rgb(0xE3, 0xF5, 0xEA),
            Color32::from_rgb(0xE1, 0xF3, 0xE8),
        ),
        (ThemeKind::Light, AccentColor::Yellow) => (
            Color32::from_rgb(0xC9, 0x9A, 0x16),
            Color32::from_rgb(0xA3, 0x7C, 0x0F),
            Color32::from_rgb(0xF9, 0xF1, 0xD8),
            Color32::from_rgb(0xF8, 0xEF, 0xD4),
        ),
        (ThemeKind::Light, AccentColor::Orange) => (
            Color32::from_rgb(0xD9, 0x74, 0x1B),
            Color32::from_rgb(0xB4, 0x5F, 0x12),
            Color32::from_rgb(0xFC, 0xEB, 0xDD),
            Color32::from_rgb(0xFB, 0xE8, 0xD8),
        ),
        (ThemeKind::Light, AccentColor::Turquoise) => (
            Color32::from_rgb(0x15, 0x9A, 0x9E),
            Color32::from_rgb(0x0F, 0x7D, 0x80),
            Color32::from_rgb(0xDF, 0xF3, 0xF3),
            Color32::from_rgb(0xDD, 0xF1, 0xF1),
        ),
        (ThemeKind::Dark, AccentColor::Blue) => (
            Color32::from_rgb(0x4B, 0x8B, 0xF5),
            Color32::from_rgb(0x8C, 0xB6, 0xFA),
            Color32::from_rgb(0x24, 0x36, 0x57),
            Color32::from_rgb(0x23, 0x3A, 0x63),
        ),
        (ThemeKind::Dark, AccentColor::Red) => (
            Color32::from_rgb(0xEF, 0x6A, 0x66),
            Color32::from_rgb(0xF6, 0x9D, 0x99),
            Color32::from_rgb(0x4A, 0x26, 0x25),
            Color32::from_rgb(0x4A, 0x2B, 0x29),
        ),
        (ThemeKind::Dark, AccentColor::Green) => (
            Color32::from_rgb(0x54, 0xC9, 0x8A),
            Color32::from_rgb(0x8B, 0xE0, 0xB0),
            Color32::from_rgb(0x1E, 0x40, 0x2F),
            Color32::from_rgb(0x1F, 0x42, 0x30),
        ),
        (ThemeKind::Dark, AccentColor::Yellow) => (
            Color32::from_rgb(0xE3, 0xC3, 0x4F),
            Color32::from_rgb(0xF0, 0xD9, 0x7A),
            Color32::from_rgb(0x45, 0x3C, 0x1E),
            Color32::from_rgb(0x43, 0x3B, 0x20),
        ),
        (ThemeKind::Dark, AccentColor::Orange) => (
            Color32::from_rgb(0xF0, 0x94, 0x42),
            Color32::from_rgb(0xF7, 0xB8, 0x7C),
            Color32::from_rgb(0x4A, 0x31, 0x1C),
            Color32::from_rgb(0x49, 0x32, 0x1F),
        ),
        (ThemeKind::Dark, AccentColor::Turquoise) => (
            Color32::from_rgb(0x4C, 0xC9, 0xC0),
            Color32::from_rgb(0x85, 0xE1, 0xD9),
            Color32::from_rgb(0x1C, 0x41, 0x40),
            Color32::from_rgb(0x1E, 0x42, 0x41),
        ),
    };
    AccentTokens {
        accent: base,
        accent_text,
        accent_soft,
        selected,
        on_accent: contrasting_text(base),
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
    /// Text/icon colour drawn on top of a filled accent button.
    pub on_accent: Color32,
    pub field: Color32,
    pub page_shadow: Color32,
    pub radius: u8,
}

impl Tokens {
    /// The original blue palette, used as a fallback before the first apply.
    pub fn for_kind(kind: ThemeKind) -> Self {
        Self::for_kind_with_accent(kind, AccentColor::Blue)
    }

    /// Tokens for `kind`, with the accent tones chosen by the user.
    pub fn for_kind_with_accent(kind: ThemeKind, accent: AccentColor) -> Self {
        let a = accent_tokens(kind, accent);
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
                selected: a.selected,
                accent: a.accent,
                accent_text: a.accent_text,
                accent_soft: a.accent_soft,
                on_accent: a.on_accent,
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
                selected: a.selected,
                accent: a.accent,
                accent_text: a.accent_text,
                accent_soft: a.accent_soft,
                on_accent: a.on_accent,
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
        self.on_accent = with_alpha(self.on_accent, alpha);
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

    /// A voice trace colour. V1 stays in the original blue regardless of the
    /// chosen accent, so its level meter and trace remain easy to identify.
    pub fn voice(&self, index: usize) -> Color32 {
        const VOICES: [Color32; 6] = [
            Color32::from_rgb(0x54, 0x9A, 0xFF),
            Color32::from_rgb(0x2F, 0xC5, 0xB0),
            Color32::from_rgb(0xF2, 0xB3, 0x3C),
            Color32::from_rgb(0xF2, 0x6D, 0xA6),
            Color32::from_rgb(0xA0, 0x7B, 0xF5),
            Color32::from_rgb(0x6B, 0xD6, 0x7C),
        ];
        // `accent.a()` carries the mini player's opacity after `with_opacity`,
        // so both the fixed blue and the other voice colours follow it.
        if index == 0 {
            with_alpha(
                accent_tokens(self.kind, AccentColor::Blue).accent,
                self.accent.a(),
            )
        } else {
            with_alpha(VOICES[index % VOICES.len()], self.accent.a())
        }
    }
}

/// Install the tokens and a complete egui style for `kind` and `accent`.
pub fn apply(ctx: &egui::Context, kind: ThemeKind, accent: AccentColor) {
    let t = Tokens::for_kind_with_accent(kind, accent);
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

#[cfg(test)]
mod tests {
    use super::{AccentColor, Color32, ThemeKind, Tokens};

    #[test]
    fn blue_is_the_default_and_keeps_the_original_tones() {
        assert_eq!(AccentColor::default(), AccentColor::Blue);
        let light = Tokens::for_kind(ThemeKind::Light);
        assert_eq!(light.accent, Color32::from_rgb(0x1B, 0x63, 0xE0));
        assert_eq!(light.accent_text, Color32::from_rgb(0x17, 0x55, 0xC4));
        assert_eq!(light.accent_soft, Color32::from_rgb(0xE3, 0xEC, 0xFD));
        assert_eq!(light.selected, Color32::from_rgb(0xE6, 0xEE, 0xFD));
        let dark = Tokens::for_kind(ThemeKind::Dark);
        assert_eq!(dark.accent, Color32::from_rgb(0x4B, 0x8B, 0xF5));
        assert_eq!(dark.accent_text, Color32::from_rgb(0x8C, 0xB6, 0xFA));
        assert_eq!(dark.accent_soft, Color32::from_rgb(0x24, 0x36, 0x57));
        assert_eq!(dark.selected, Color32::from_rgb(0x23, 0x3A, 0x63));
    }

    #[test]
    fn accent_config_keys_round_trip_and_fall_back_to_blue() {
        for accent in AccentColor::ALL {
            assert_eq!(AccentColor::from_config(accent.config_key()), accent);
        }
        assert_eq!(AccentColor::from_config("purple"), AccentColor::Blue);
        assert_eq!(AccentColor::from_config(""), AccentColor::Blue);
    }

    #[test]
    fn every_accent_has_distinct_tones_and_readable_button_text() {
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let mut accents = Vec::new();
            for accent in AccentColor::ALL {
                let tokens = Tokens::for_kind_with_accent(kind, accent);
                accents.push(tokens.accent);
                if accent != AccentColor::Blue {
                    assert_ne!(tokens.selected, Tokens::for_kind(kind).selected);
                    assert_ne!(tokens.accent_soft, Tokens::for_kind(kind).accent_soft);
                }
            }
            let unique: std::collections::HashSet<_> = accents.into_iter().collect();
            assert_eq!(unique.len(), AccentColor::ALL.len());
        }
        let blue = Tokens::for_kind_with_accent(ThemeKind::Dark, AccentColor::Blue);
        let yellow = Tokens::for_kind_with_accent(ThemeKind::Dark, AccentColor::Yellow);
        assert_eq!(blue.on_accent, Color32::WHITE);
        assert_ne!(yellow.on_accent, Color32::WHITE);
    }

    #[test]
    fn voice_one_keeps_its_blue_tone_for_every_accent() {
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let original = Tokens::for_kind(kind).voice(0);
            for accent in AccentColor::ALL {
                assert_eq!(Tokens::for_kind_with_accent(kind, accent).voice(0), original);
            }
        }
    }
}
