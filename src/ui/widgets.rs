//! Small custom widgets: cards, chips, segmented controls, badges.

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Response, RichText, Sense, Stroke, Ui, Vec2};

use super::theme::{Tokens, tokens};

/// A rounded card with a title row. `right` adds widgets at the right end
/// of the title row. `collapsible: Some(default_open)` makes the title
/// toggle the body; the state is remembered by `id`.
pub fn card<R>(
    ui: &mut Ui,
    id: &str,
    icon: &str,
    title: &str,
    collapsible: Option<bool>,
    right: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let t = tokens(ui.ctx());
    let mut out = None;
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin { left: 12, right: 12, top: 10, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let state_id = ui.make_persistent_id(("card", id));
            let mut open = ui.ctx().data_mut(|d| *d.get_persisted_mut_or(state_id, collapsible.unwrap_or(true)));
            let collapsible = collapsible.is_some();
            ui.horizontal(|ui| {
                let label = super::theme::with_icon(icon, title);
                let text = RichText::new(label).size(14.5).strong().color(t.text);
                let resp = if collapsible {
                    let arrow = if open { "▾" } else { "▸" };
                    ui.add(egui::Label::new(RichText::new(arrow).color(t.faint)).sense(Sense::click()))
                        .union(ui.add(egui::Label::new(text).sense(Sense::click())))
                } else {
                    ui.label(text)
                };
                if collapsible && resp.clicked() {
                    open = !open;
                    ui.ctx().data_mut(|d| d.insert_persisted(state_id, open));
                }
                if collapsible {
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
            });
            if open || !collapsible {
                ui.add_space(4.0);
                out = Some(body(ui));
            }
        });
    ui.add_space(8.0);
    out
}

/// Toggle chip with an optional colour swatch; returns the response.
pub fn chip(ui: &mut Ui, on: &mut bool, text: &str, swatch: Option<Color32>) -> Response {
    let t = tokens(ui.ctx());
    let font = FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, t.text);
    let sw = if swatch.is_some() { 14.0 } else { 0.0 };
    let size = Vec2::new(galley.size().x + 22.0 + sw, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let (fill, stroke, fg) = chip_colors(&t, *on, resp.hovered());
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(13), fill, stroke, egui::StrokeKind::Inside);
    let mut x = rect.min.x + 11.0;
    if let Some(c) = swatch {
        let c = if *on { c } else { c.gamma_multiply(0.45) };
        p.circle_filled(egui::pos2(x + 4.0, rect.center().y), 4.5, c);
        x += sw;
    }
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn chip_colors(t: &Tokens, on: bool, hovered: bool) -> (Color32, Stroke, Color32) {
    match (on, hovered) {
        (true, _) => (t.accent_soft, Stroke::new(1.0, t.accent.gamma_multiply(0.7)), t.accent_text),
        (false, true) => (t.card, Stroke::new(1.0, t.accent.gamma_multiply(0.5)), t.text),
        (false, false) => (t.card, Stroke::new(1.0, t.card_border), t.muted),
    }
}

/// Segmented control. Returns true when the selection changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str, &str)]) -> bool {
    let t = tokens(ui.ctx());
    let font = FontId::proportional(13.0);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label, _)| ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text))
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 20.0).collect();
    let total = Vec2::new(widths.iter().sum::<f32>() + 4.0, 28.0);
    let (rect, base) = ui.allocate_exact_size(total, Sense::hover());
    let track = if t.dark { Color32::from_rgb(0x14, 0x16, 0x1d) } else { Color32::from_rgb(0xe6, 0xe9, 0xf0) };
    ui.painter().rect_filled(rect, CornerRadius::same(8), track);
    let mut x = rect.min.x + 2.0;
    let mut changed = false;
    for (i, ((val, _, tip), galley)) in options.iter().zip(galleys).enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.min.y + 2.0), Vec2::new(widths[i], 24.0));
        let resp = ui.interact(r, base.id.with(("seg", i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let resp = if tip.is_empty() { resp } else { resp.on_hover_text(*tip) };
        if resp.clicked() && *value != *val {
            *value = *val;
            changed = true;
        }
        let selected = *value == *val;
        if selected {
            ui.painter().rect(r, CornerRadius::same(6), t.card, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(6), t.card.gamma_multiply(0.5));
        }
        let fg = if selected { t.accent_text } else { t.muted };
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, fg);
        x += widths[i];
    }
    changed
}

/// Small rounded status badge.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), color.gamma_multiply(0.14));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// Floating panel drawn over the 3D canvas.
pub fn overlay_frame(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.overlay)
        .stroke(Stroke::new(1.0, t.overlay_border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 7))
        .shadow(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(if t.dark { 90 } else { 22 }) })
}

/// Muted caption text.
pub fn caption(ui: &mut Ui, text: &str) {
    let t = tokens(ui.ctx());
    ui.label(RichText::new(text).size(12.0).color(t.muted));
}

/// Draw text centred at a point (helper for canvases).
pub fn text_at(ui: &Ui, pos: egui::Pos2, text: &str, size: f32, color: Color32) {
    ui.painter().text(pos, Align2::CENTER_CENTER, text, FontId::proportional(size), color);
}
