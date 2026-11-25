// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

use super::*;
pub(super) fn mode_tab(ui: &mut egui::Ui, t: &Tokens, label: &str, selected: bool) -> Response {
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

pub(super) fn icon_button(
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

/// Like [`primary_button`] but square and icon-only, for a compact accent action.
pub(super) fn primary_icon_button(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    size: f32,
    tip: &str,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let fill = if resp.hovered() {
        t.accent.gamma_multiply(1.08)
    } else {
        t.accent
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    let glyph = size - size * 0.54;
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(glyph)),
        icon,
        t.on_accent,
        (size * 0.07).clamp(1.2, 1.9),
    );
    theme::hand(resp).on_hover_text(tip)
}

/// Like [`icon_button`] but with an explicit glyph size and stroke width, so a
/// compact hit area can still draw an icon as large as the transport arrows.
pub(super) fn icon_button_glyph(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    size: f32,
    glyph: f32,
    stroke: f32,
    selected: bool,
    tip: &str,
) -> Response {
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let down = enabled && resp.is_pointer_button_down_on();
    if enabled && (selected || resp.hovered() || down) {
        let fill = if selected {
            t.accent_soft
        } else if down {
            t.pressed
        } else {
            t.hover
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    }
    let color = if !enabled {
        t.text_faint
    } else if selected {
        t.accent
    } else {
        t.text
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(glyph)),
        icon,
        color,
        stroke,
    );
    if enabled {
        theme::hand(resp).on_hover_text(tip)
    } else {
        resp
    }
}

/// [`icon_button`] that can be drawn and treated as disabled. The caller must
/// also check its own `enabled` flag before acting on the click.
pub(super) fn icon_button_enabled(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: &str,
    size: f32,
    selected: bool,
    enabled: bool,
    tip: &str,
) -> Response {
    if enabled {
        icon_button(ui, t, icon, size, selected, tip)
    } else {
        ui.add_enabled_ui(false, |ui| icon_button(ui, t, icon, size, selected, tip))
            .inner
    }
}

pub(super) fn transport_button(
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
        paint_icon(ui.painter(), icon_rect, icon, t.on_accent, 1.8);
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

pub(super) fn search_box(
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

pub(super) fn tool_row(
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

pub(super) fn playlist_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    idx: usize,
    name: &str,
    source: &str,
    subsong: Option<u16>,
    selected: bool,
    draggable: bool,
) -> Response {
    let sense = if draggable {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), sense);
    let fill = if resp.dragged() {
        t.pressed
    } else if selected {
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
    // A drag handle needs a little room at the right edge, so the text stops
    // short of it.
    let gutter = if draggable { 22.0 } else { 0.0 };
    let name_width = (rect.width() - 72.0 - gutter).max(40.0);
    paint_text_elided(
        ui,
        rect.left_center() + vec2(34.0, -8.0),
        Align2::LEFT_CENTER,
        &format!("{:02}. {name}", idx + 1),
        theme::medium(12.5),
        t.text,
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
            rect.right_center() - vec2(10.0 + gutter, 0.0),
            Align2::RIGHT_CENTER,
            right,
            theme::regular(10.0),
            t.text_faint,
        );
    }
    if draggable {
        paint_icon(
            ui.painter(),
            Rect::from_center_size(rect.right_center() - vec2(13.0, 0.0), vec2(14.0, 18.0)),
            "grip",
            if resp.dragged() { t.accent } else { t.text_faint },
            1.2,
        );
        if resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            resp
        } else {
            resp.on_hover_cursor(CursorIcon::Grab)
        }
    } else {
        theme::hand(resp)
    }
}

pub(super) fn result_row(
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

pub(super) fn tool_card(
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

pub(super) fn primary_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> Response {
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
        t.on_accent,
    );
    theme::hand(resp)
}

pub(super) fn secondary_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> Response {
    secondary_button_with_font(ui, t, label, theme::medium(12.5))
}

pub(super) fn secondary_button_with_font(
    ui: &mut egui::Ui,
    t: &Tokens,
    label: &str,
    font: FontId,
) -> Response {
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

pub(super) fn ghost_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> Response {
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

pub(super) fn segmented(
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

/// Six labelled colour chips. The swatch shows the actual hue; the selected
/// chip is outlined with the live accent so the choice is visible immediately.
pub(super) fn accent_picker(
    ui: &mut egui::Ui,
    t: &Tokens,
    current: AccentColor,
) -> Option<AccentColor> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        for accent in AccentColor::ALL {
            let font = theme::medium(11.5);
            let label = accent.label();
            let dot = 12.0;
            let width = 12.0 + 6.0 + text_width(ui, label, &font) + 16.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
            let selected = accent == current;
            let fill = if selected {
                t.accent_soft
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::TRANSPARENT
            };
            if fill != Color32::TRANSPARENT {
                ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
            }
            if selected {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(7),
                    Stroke::new(1.0_f32, t.accent),
                    StrokeKind::Inside,
                );
            }
            ui.painter().circle_filled(
                pos2(rect.left() + 12.0, rect.center().y),
                dot * 0.5,
                accent.swatch(),
            );
            ui.painter().text(
                pos2(rect.left() + 12.0 + 6.0 + dot, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                font,
                if selected { t.accent_text } else { t.text_muted },
            );
            if theme::hand(resp).clicked() {
                chosen = Some(accent);
            }
        }
    });
    chosen
}

pub(super) fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, color: Color32) {
    let font = theme::medium(10.0);
    let width = text_width(ui, text, &font) + 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

pub(super) fn property_row(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &str) {
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

pub(super) fn property_row_right(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &str) {
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

/// Artwork size, the gap between it and the text column, and the header height
/// that leaves room for the name, tagline, version and author block.
pub(super) const ABOUT_ICON: f32 = 88.0;
pub(super) const ABOUT_GAP: f32 = 16.0;
pub(super) const ABOUT_HEADER: f32 = 96.0;
pub(super) const ABOUT_WEBSITE: &str = "https://github.com/emartisoft/SidCraft";

/// About dialog link row: a quiet label on the left and a clickable URL that
/// hugs the right edge.
pub(super) fn about_link_row(ui: &mut egui::Ui, t: &Tokens, label: &str, url: &str) -> Response {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .font(theme::regular(11.5))
                .color(t.text_faint),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(url)
                        .font(theme::regular(11.5))
                        .color(t.accent_text)
                        .underline(),
                )
                .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Open in your default browser")
        })
        .inner
    })
    .inner
}

/// About dialog licence row: the component on the left, its licence on the
/// right, so the names and the SPDX identifiers form two clean columns.
pub(super) fn about_license_row(ui: &mut egui::Ui, t: &Tokens, component: &str, license: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(component)
                .font(theme::regular(11.0))
                .color(t.text_muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(license)
                    .font(theme::regular(11.0))
                    .color(t.text_faint),
            );
        });
    });
}

/// Opens `url` in the user's default browser through the OS protocol handler,
/// so the About link behaves like a normal hyperlink.
pub(super) fn open_url(url: &str) {
    #[cfg(windows)]
    {
        // `rundll32` is a GUI process, so this does not flash a console window
        // the way spawning a console program from a GUI app can.
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// A one pixel rule used to group the About dialog into header, details and
/// footer without the heavier default separator.
pub(super) fn hairline(ui: &mut egui::Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.border);
}

pub(super) fn spectrum_color(t: &Tokens, value: f32) -> Color32 {
    let hot = Color32::from_rgb(0xFF, 0xC4, 0x5C);
    mix_color(t.accent, hot, value.clamp(0.0, 1.0).powf(1.6))
}

pub(super) fn mix_color(a: Color32, b: Color32, t: f32) -> Color32 {
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

pub(super) fn heat_color(value: f32) -> Color32 {
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

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn stage_color(stage: u8) -> Color32 {
    match stage {
        0 => Color32::from_rgb(0x6B, 0xD6, 0x7C),
        2 => Color32::from_rgb(0xF2, 0x6D, 0xA6),
        _ => Color32::from_rgb(0xF2, 0xB3, 0x3C),
    }
}

pub(super) fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(note) / 12 - 1;
    format!("{}{}", NAMES[usize::from(note % 12)], octave)
}

pub(super) fn section_title(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    ui.label(
        RichText::new(label.to_uppercase())
            .font(theme::semibold(10.5))
            .color(t.text_faint),
    );
}

pub(super) fn voice_meters(
    ui: &mut egui::Ui,
    t: &Tokens,
    levels: &[f32],
    peaks: &[f32],
    mutes: &[bool],
) -> Option<usize> {
    let mut toggled = None;
    for (i, level) in levels.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let muted = mutes.get(i).copied().unwrap_or(false);
            // A compact speaker switch sits beside the bar; muting only
            // silences this voice, the rest of the mix keeps playing.
            if icon_button_glyph(
                ui,
                t,
                if muted { "speaker-off" } else { "speaker" },
                16.0,
                12.0,
                1.4,
                muted,
                if muted { "Unmute voice" } else { "Mute voice" },
            )
            .clicked()
            {
                toggled = Some(i);
            }
            let tint = if muted {
                t.voice(i).gamma_multiply(0.4)
            } else {
                t.voice(i)
            };
            ui.label(
                RichText::new(format!("V{}", i + 1))
                    .font(theme::mono(9.5))
                    .color(tint),
            );
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
            let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
            ui.painter().rect_filled(track, CornerRadius::same(2), t.pressed);
            let fill = Rect::from_min_size(
                track.min,
                vec2(track.width() * level.clamp(0.0, 1.0), track.height()),
            );
            ui.painter().rect_filled(fill, CornerRadius::same(2), tint);
            let peak = peaks.get(i).copied().unwrap_or(0.0);
            let x = track.left() + track.width() * peak.clamp(0.0, 1.0);
            ui.painter().line_segment(
                [pos2(x, track.top() - 2.0), pos2(x, track.bottom() + 2.0)],
                Stroke::new(1.5_f32, if muted { tint } else { t.text }),
            );
        });
    }
    toggled
}

pub(super) fn seek_bar(ui: &mut egui::Ui, t: &Tokens, fraction: f32) -> Option<f32> {
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

pub(super) fn volume_bar(ui: &mut egui::Ui, t: &Tokens, fraction: f32, width: f32) -> Option<f32> {
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

pub(super) fn settings_card(
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

pub(super) fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

pub(super) fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x)
}

pub(super) fn paint_text_elided(
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

