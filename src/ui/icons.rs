// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

use super::*;
pub(super) fn paint_theme_icon(
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

pub(super) fn paint_icon(painter: &egui::Painter, rect: Rect, name: &str, color: Color32, width: f32) {
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
            // Two crossing tracks with solid arrowheads.
            let left = r.left() + s * 0.04;
            let knee = r.left() + s * 0.36;
            let tip_x = r.right() - s * 0.04;
            let top = r.top() + s * 0.26;
            let bottom = r.bottom() - s * 0.26;

            let arrow = |tip: Pos2, dir: Vec2| {
                let d = dir.normalized();
                let n = vec2(-d.y, d.x);
                let base = tip - d * (s * 0.24);
                poly(vec![
                    tip,
                    base + n * (s * 0.12),
                    base - n * (s * 0.12),
                ]);
            };

            // Lower-left to upper-right track.
            line(pos2(left, bottom), pos2(knee, bottom));
            line(pos2(knee, bottom), pos2(tip_x, top));
            arrow(pos2(tip_x, top), vec2(tip_x - knee, top - bottom));

            // Upper-left to lower-right track.
            line(pos2(left, top), pos2(knee, top));
            line(pos2(knee, top), pos2(tip_x, bottom));
            arrow(pos2(tip_x, bottom), vec2(tip_x - knee, bottom - top));
        }
        "repeat" => {
            // Rectangular loop: top track pointing right, bottom track
            // pointing left, each with a solid arrowhead.
            let left = r.left() + s * 0.04;
            let right = r.right() - s * 0.04;
            let top = r.top() + s * 0.24;
            let bottom = r.bottom() - s * 0.24;
            let stub = s * 0.26;
            let head = s * 0.2;

            // Top run.
            line(pos2(left, top + stub), pos2(left, top));
            line(pos2(left, top), pos2(right, top));
            poly(vec![
                pos2(right - s * 0.02, top),
                pos2(right - s * 0.02 - head, top - head * 0.7),
                pos2(right - s * 0.02 - head, top + head * 0.7),
            ]);

            // Bottom run.
            line(pos2(right, bottom - stub), pos2(right, bottom));
            line(pos2(right, bottom), pos2(left, bottom));
            poly(vec![
                pos2(left + s * 0.02, bottom),
                pos2(left + s * 0.02 + head, bottom - head * 0.7),
                pos2(left + s * 0.02 + head, bottom + head * 0.7),
            ]);
        }
        "eraser" => {
            // A tilted eraser with the seam between its two halves.
            let u = vec2(1.0, -1.0).normalized() * (s * 0.3);
            let v = vec2(1.0, 1.0).normalized() * (s * 0.16);
            let corners = [c + u + v, c + u - v, c - u - v, c - u + v];
            painter.add(egui::Shape::line(
                vec![corners[0], corners[1], corners[2], corners[3], corners[0]],
                stroke,
            ));
            let seam = c - u * 0.2;
            line(seam + v, seam - v);
        }
        "trash" => {
            // Trash can with a cross: a clearer destructive-delete symbol.
            let handle = Rect::from_min_max(
                pos2(c.x - s * 0.12, r.top() + s * 0.16),
                pos2(c.x + s * 0.12, r.top() + s * 0.28),
            );
            painter.rect_stroke(handle, CornerRadius::same(1), stroke, StrokeKind::Inside);
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
                pos2(body.left() + s * 0.12, body.top() + s * 0.12),
                pos2(body.right() - s * 0.12, body.bottom() - s * 0.12),
            );
            line(
                pos2(body.right() - s * 0.12, body.top() + s * 0.12),
                pos2(body.left() + s * 0.12, body.bottom() - s * 0.12),
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
        "folder-open" => {
            // Back panel with the tab, then the front flap leaning open.
            let left = r.left() + s * 0.08;
            let right = r.right() - s * 0.04;
            let top = r.top() + s * 0.2;
            let bottom = r.bottom() - s * 0.14;
            line(pos2(left, bottom), pos2(left, top + s * 0.08));
            line(pos2(left, top + s * 0.08), pos2(left + s * 0.24, top + s * 0.08));
            line(
                pos2(left + s * 0.24, top + s * 0.08),
                pos2(left + s * 0.34, top),
            );
            line(pos2(left + s * 0.34, top), pos2(right, top));
            painter.add(egui::Shape::line(
                vec![
                    pos2(left + s * 0.02, bottom),
                    pos2(right - s * 0.1, bottom),
                    pos2(right + s * 0.04, top + s * 0.26),
                    pos2(left + s * 0.16, top + s * 0.26),
                    pos2(left + s * 0.02, bottom),
                ],
                stroke,
            ));
        }
        "pencil" => {
            // A slanted pencil: two shaft edges, the back seam and a solid tip.
            let back = pos2(r.left() + s * 0.16, r.bottom() - s * 0.16);
            let tip = pos2(r.right() - s * 0.2, r.top() + s * 0.2);
            let d = (tip - back).normalized();
            let n = vec2(-d.y, d.x) * (s * 0.09);
            line(back + n, tip + n);
            line(back - n, tip - n);
            line(back + n, back - n);
            poly(vec![tip + n, tip - n, tip + d * (s * 0.18)]);
        }
        "save" => {
            // Floppy disk: body, shutter and label.
            let body = r.shrink(s * 0.08);
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            let shutter = Rect::from_min_max(
                pos2(body.left() + s * 0.16, body.top()),
                pos2(body.right() - s * 0.16, body.top() + s * 0.22),
            );
            painter.rect_stroke(shutter, CornerRadius::same(1), stroke, StrokeKind::Inside);
            let label = Rect::from_min_max(
                pos2(body.left() + s * 0.18, body.bottom() - s * 0.3),
                pos2(body.right() - s * 0.18, body.bottom() - s * 0.05),
            );
            painter.rect_stroke(label, CornerRadius::same(1), stroke, StrokeKind::Inside);
        }
        "download" => {
            // Down arrow hovering just above a baseline: the classic download
            // glyph. The tip stops short of the line so they never intersect.
            let line_y = r.bottom() - s * 0.1;
            line(
                pos2(r.left() + s * 0.12, line_y),
                pos2(r.right() - s * 0.12, line_y),
            );
            let tip_y = line_y - s * 0.12;
            let base_y = tip_y - s * 0.2;
            line(pos2(c.x, r.top() + s * 0.06), pos2(c.x, base_y));
            poly(vec![
                pos2(c.x - s * 0.17, base_y),
                pos2(c.x + s * 0.17, base_y),
                pos2(c.x, tip_y),
            ]);
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
        "file-plus" => {
            // A file outline with a plus for adding tracks.
            let body = Rect::from_min_max(
                pos2(r.left() + s * 0.22, r.top() + s * 0.06),
                pos2(r.right() - s * 0.16, r.bottom() - s * 0.06),
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
            let p = pos2(c.x + s * 0.02, c.y + s * 0.1);
            line(pos2(p.x - s * 0.11, p.y), pos2(p.x + s * 0.11, p.y));
            line(pos2(p.x, p.y - s * 0.11), pos2(p.x, p.y + s * 0.11));
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
        "to-top" => {
            // An arrow stopping at a bar, for "jump to the top".
            line(
                pos2(c.x - s * 0.24, c.y - s * 0.28),
                pos2(c.x + s * 0.24, c.y - s * 0.28),
            );
            line(pos2(c.x - s * 0.22, c.y + s * 0.18), pos2(c.x, c.y - s * 0.06));
            line(pos2(c.x, c.y - s * 0.06), pos2(c.x + s * 0.22, c.y + s * 0.18));
        }
        "to-bottom" => {
            // The same arrow mirrored, for "jump to the bottom".
            line(
                pos2(c.x - s * 0.24, c.y + s * 0.28),
                pos2(c.x + s * 0.24, c.y + s * 0.28),
            );
            line(pos2(c.x - s * 0.22, c.y - s * 0.18), pos2(c.x, c.y + s * 0.06));
            line(pos2(c.x, c.y + s * 0.06), pos2(c.x + s * 0.22, c.y - s * 0.18));
        }
        "grip" => {
            // Two columns of dots: the drag handle of a reorderable row.
            for i in 0..3 {
                let y = c.y - s * 0.2 + i as f32 * s * 0.2;
                painter.circle_filled(pos2(c.x - s * 0.12, y), s * 0.055, color);
                painter.circle_filled(pos2(c.x + s * 0.12, y), s * 0.055, color);
            }
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
            // A crisp square, not a rounded blob, so it reads as a stop button
            // even at the transport size.
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::splat(s * 0.4)),
                CornerRadius::ZERO,
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
        "mini" => {
            // A small player frame sitting inside the full shell.
            let outer = r.shrink(s * 0.08);
            painter.rect_stroke(outer, CornerRadius::same(2), stroke, StrokeKind::Inside);
            let inner = Rect::from_min_size(
                pos2(
                    outer.left() + outer.width() * 0.14,
                    outer.bottom() - outer.height() * 0.54,
                ),
                vec2(outer.width() * 0.54, outer.height() * 0.38),
            );
            painter.rect_filled(inner, CornerRadius::same(1), color);
        }
        "maximize" => {
            // Four outward corners: leave the compact player.
            let b = r.shrink(s * 0.1);
            let arm = s * 0.22;
            for (x, sx) in [(b.left(), 1.0), (b.right(), -1.0)] {
                for (y, sy) in [(b.top(), 1.0), (b.bottom(), -1.0)] {
                    line(pos2(x + sx * arm, y), pos2(x, y));
                    line(pos2(x, y), pos2(x, y + sy * arm));
                }
            }
        }
        _ => {
            painter.circle_stroke(c, s * 0.32, stroke);
        }
    }
}

