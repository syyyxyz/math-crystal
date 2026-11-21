//! Small custom widgets: cards, chips, segmented controls, badges.

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Response, RichText, Sense, Stroke, Ui, Vec2};

use super::theme::{Tokens, tokens};

/// A rounded, collapsible card. Clicking anywhere on the title row toggles
/// the body; `right` adds widgets at the right end of the title row (they
/// take their own clicks). The open state is kept by `id` for the session.
pub fn card<R>(
    ui: &mut Ui,
    id: &str,
    icon: &str,
    title: &str,
    default_open: bool,
    right: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let t = tokens(ui.ctx());
    let mut out = None;
    let state_id = ui.make_persistent_id(("card", id));
    // Not persisted: every start begins with the default (mostly folded) layout.
    let mut open = ui.ctx().data_mut(|d| *d.get_temp_mut_or(state_id, default_open));
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin { left: 6, right: 8, top: 4, bottom: if open { 12 } else { 4 } })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let row = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 30.0));
            let resp = ui.interact(row, state_id.with("header"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                ui.painter().rect_filled(row, CornerRadius::same(7), t.accent_soft.gamma_multiply(0.6));
            }
            if resp.clicked() {
                open = !open;
                ui.ctx().data_mut(|d| d.insert_temp(state_id, open));
            }
            ui.allocate_ui_with_layout(row.size(), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_size(row.size());
                ui.add_space(4.0);
                let arrow = if super::theme::icons_available() {
                    if open { super::theme::icon::CHEVRON_DOWN } else { super::theme::icon::CHEVRON_RIGHT }
                } else if open {
                    "▾"
                } else {
                    "▸"
                };
                ui.add(egui::Label::new(RichText::new(arrow).color(t.faint)).selectable(false));
                let label = super::theme::with_icon(icon, title);
                ui.add(egui::Label::new(RichText::new(label).size(14.0).strong().color(if open { t.text } else { t.muted })).selectable(false));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
            });
            if open {
                ui.add_space(4.0);
                // Body aligned with the title text, not the chevron.
                egui::Frame::new().inner_margin(Margin { left: 6, right: 4, top: 0, bottom: 0 }).show(ui, |ui| {
                    out = Some(body(ui));
                });
            }
        });
    ui.add_space(5.0);
    out
}

/// Compact "show in the view" toggle (eye icon). Returns true when changed.
pub fn eye(ui: &mut Ui, on: &mut bool, tip: &str) -> bool {
    let t = tokens(ui.ctx());
    if !super::theme::icons_available() {
        return ui.checkbox(on, "").on_hover_text(tip).changed();
    }
    let ic = if *on { super::theme::icon::EYE } else { super::theme::icon::EYE_OFF };
    let color = if *on { t.accent_text } else { t.faint };
    let resp = ui
        .add(egui::Button::new(RichText::new(ic).size(16.0).color(color)).frame(false).min_size(Vec2::new(26.0, 22.0)))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip);
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
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

/// What [`tab_bar`] did this frame.
#[derive(Default, Clone, Copy)]
pub struct TabChange {
    /// Another tab was chosen.
    pub selected: bool,
    /// The tabs were reordered (dragged, or reset from the context menu).
    pub reordered: bool,
}

/// Segmented tabs that can be dragged into another order. `order` holds
/// the tabs as shown; `default_order` is offered on right-click.
pub fn tab_bar<T: PartialEq + Copy + std::hash::Hash + std::fmt::Debug>(
    ui: &mut Ui,
    value: &mut T,
    order: &mut Vec<T>,
    default_order: &[T],
    label: impl Fn(T) -> (String, &'static str),
) -> TabChange {
    let t = tokens(ui.ctx());
    let font = FontId::proportional(13.0);
    let mut out = TabChange::default();
    let galleys: Vec<_> = order.iter().map(|&v| ui.painter().layout_no_wrap(label(v).0, font.clone(), t.text)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 20.0).collect();
    let total = Vec2::new(widths.iter().sum::<f32>() + 4.0, 28.0);
    let (rect, base) = ui.allocate_exact_size(total, Sense::hover());
    let track = if t.dark { Color32::from_rgb(0x14, 0x16, 0x1d) } else { Color32::from_rgb(0xe6, 0xe9, 0xf0) };
    ui.painter().rect_filled(rect, CornerRadius::same(8), track);
    // Slots in the current order.
    let mut slots = Vec::with_capacity(order.len());
    let mut x = rect.min.x + 2.0;
    for w in &widths {
        slots.push(egui::Rect::from_min_size(egui::pos2(x, rect.min.y + 2.0), Vec2::new(*w, 24.0)));
        x += w;
    }
    let grab_id = base.id.with("tab_grab");
    let pointer = ui.input(|i| i.pointer.interact_pos());
    // Dragged tab: index, where it is drawn, centre for ordering.
    let mut dragged: Option<(usize, egui::Rect, f32)> = None;
    for (i, &v) in order.iter().enumerate() {
        let id = base.id.with(("tab", v));
        let resp = ui.interact(slots[i], id, Sense::click_and_drag());
        // Where in the tab it was grabbed: the press, not where the drag
        // was recognised (that can be far off after a quick move).
        if resp.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            ui.ctx().data_mut(|d| d.insert_temp(grab_id, p.x - slots[i].min.x));
        }
        if resp.dragged()
            && let Some(p) = pointer
        {
            let grab = ui.ctx().data(|d| d.get_temp::<f32>(grab_id)).unwrap_or(slots[i].width() / 2.0);
            // Drawn inside the bar; ordered by where the pointer really is,
            // so dragging past either end reaches the first or last slot.
            let free = p.x - grab;
            let left = free.clamp(rect.min.x + 2.0, rect.max.x - 2.0 - slots[i].width());
            dragged = Some((i, slots[i].translate(Vec2::new(left - slots[i].min.x, 0.0)), free + slots[i].width() / 2.0));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else {
            let tip = label(v).1;
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
            if resp.clicked() && *value != v {
                *value = v;
                out.selected = true;
            }
            resp.context_menu(|ui| {
                let reset = ui.add_enabled(order.as_slice() != default_order, egui::Button::new(crate::i18n::tr("Reset tab order", "恢复默认顺序")));
                if reset.clicked() {
                    out.reordered = true;
                    ui.close();
                }
            });
        }
    }
    if out.reordered {
        *order = default_order.to_vec();
        return out;
    }
    let draw = |ui: &Ui, r: egui::Rect, k: usize, lifted: bool| {
        let v = order[k];
        let selected = *value == v;
        let hovered = ui.rect_contains_pointer(r);
        if lifted {
            ui.painter().rect(r, CornerRadius::same(6), t.card, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        } else if selected {
            ui.painter().rect(r, CornerRadius::same(6), t.card, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        } else if hovered {
            ui.painter().rect_filled(r, CornerRadius::same(6), t.card.gamma_multiply(0.5));
        }
        let fg = if selected { t.accent_text } else { t.muted };
        ui.painter().galley(r.center() - galleys[k].size() / 2.0, galleys[k].clone(), fg);
    };
    for (k, slot) in slots.iter().enumerate() {
        if dragged.is_none_or(|(d, _, _)| d != k) {
            draw(ui, *slot, k, false);
        }
    }
    if let Some((k, r, c)) = dragged {
        // A gap where the tab came from, the tab itself floating on top.
        ui.painter().rect_filled(slots[k].shrink(1.0), CornerRadius::same(6), track.gamma_multiply(0.6));
        draw(ui, r, k, true);
        // Move it to the slot whose neighbours' centres it lies between
        // (any distance in one frame: a quick drag still lands right).
        let j = (0..order.len()).filter(|&m| m != k && slots[m].center().x < c).count();
        if j != k {
            let v = order.remove(k);
            order.insert(j, v);
            out.reordered = true;
        }
        if out.reordered {
            ui.ctx().request_repaint();
        }
    }
    out
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

/// Room for a top-left canvas overlay next to the top-right `toolbar`: the
/// whole canvas, or below the toolbar when the two would overlap. The
/// overlay's width is remembered from the previous frame under `id`; pass
/// what was shown to [`remember_overlay`].
pub fn top_left_room(ui: &Ui, inner: egui::Rect, toolbar: egui::Rect, id: egui::Id) -> egui::Rect {
    let width = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(260.0);
    let mut room = inner;
    if toolbar.left() < inner.left() + width + 8.0 {
        room.min.y = toolbar.bottom() + 8.0;
    }
    room
}

pub fn remember_overlay(ui: &Ui, id: egui::Id, shown: egui::Rect) {
    ui.ctx().data_mut(|d| d.insert_temp(id, shown.width()));
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

/// One toggle in a [`layer_bar`]: value, full label, short label, swatch, tooltip.
pub type LayerChip<'a> = (&'a mut bool, &'a str, &'a str, Option<Color32>, &'a str);

/// Layer toggles centred at the bottom of `inner`. Folded into a single
/// "Layers" chip (showing how many are on) unless `open`; unfolded, the
/// labels shorten when the row would not fit.
pub fn layer_bar(ui: &mut Ui, inner: egui::Rect, open: &mut bool, chips: Vec<LayerChip>) {
    use super::theme::{icon, with_icon};
    use crate::i18n::tr;
    let t = tokens(ui.ctx());
    let active = chips.iter().filter(|c| *c.0).count();
    let toggle_label = if *open {
        with_icon(icon::CHEVRON_DOWN, tr("Layers", "图层"))
    } else if active > 0 {
        format!("{}  {active}", with_icon(icon::LAYERS, tr("Layers", "图层")))
    } else {
        with_icon(icon::LAYERS, tr("Layers", "图层"))
    };
    let measure = |ui: &Ui, text: &str, sw: bool| ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), Color32::WHITE).size().x + 22.0 + if sw { 14.0 } else { 0.0 };
    let toggle_w = measure(ui, &toggle_label, false) + 6.0;
    let width_of = |short: bool| toggle_w + chips.iter().map(|c| measure(ui, if short { c.2 } else { c.1 }, c.3.is_some()) + 6.0).sum::<f32>() + 10.0;
    let short = width_of(false) > inner.width() - 8.0;
    let w = if *open { width_of(short).min(inner.width()) } else { toggle_w + 10.0 };
    let bottom = egui::Rect::from_center_size(egui::pos2(inner.center().x, inner.max.y - 20.0), Vec2::new(w + 2.0, 40.0));
    let mut bc = ui.new_child(egui::UiBuilder::new().max_rect(bottom).layout(egui::Layout::left_to_right(egui::Align::Center)));
    overlay_frame(&t).inner_margin(Margin::symmetric(8, 6)).show(&mut bc, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut shown = *open;
        chip(ui, &mut shown, &toggle_label, None).on_hover_text(if *open { tr("Hide the layer list", "收起图层列表") } else { tr("Show or hide parts of the drawing", "显示或隐藏图中的元素") });
        if shown != *open {
            *open = shown;
            return;
        }
        if !*open {
            return;
        }
        for (v, full, brief, color, tip) in chips {
            let label = if short { brief } else { full };
            chip(ui, v, label, color).on_hover_text(if short { format!("{full}: {tip}") } else { tip.to_string() });
        }
    });
}
