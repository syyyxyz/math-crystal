//! Parameter sliders shared by the workspaces: every letter in a formula
//! that is not a variable gets a slider that can also sweep on its own.

use egui::{RichText, Ui};

use super::theme::{icon, icons_available, tokens};
use super::widgets::{caption, card};
use crate::i18n::tr;

pub struct Param {
    pub name: String,
    pub value: f64,
    pub lo: f64,
    pub hi: f64,
    /// Sweeping back and forth between `lo` and `hi`.
    pub playing: bool,
    dir: f64,
}

pub struct Params {
    pub items: Vec<Param>,
    /// Seconds for one sweep from `lo` to `hi`.
    pub seconds: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self { items: Vec::new(), seconds: 4.0 }
    }
}

/// What [`Params::ui`] did this frame.
#[derive(Default, Clone, Copy)]
pub struct Change {
    /// Some value moved: re-evaluate.
    pub changed: bool,
    /// A drag ended, a value was typed or playback was toggled: a good
    /// moment for slower follow-ups such as refitting ranges.
    pub settled: bool,
}

impl Params {
    /// Match the parameters of a newly parsed formula; values and ranges of
    /// names that are still there are kept.
    pub fn sync(&mut self, names: &[String]) {
        let mut old = std::mem::take(&mut self.items);
        for name in names {
            let item = match old.iter().position(|p| &p.name == name) {
                Some(k) => old.swap_remove(k),
                None => Param { name: name.clone(), value: 1.0, lo: -5.0, hi: 5.0, playing: false, dir: 1.0 },
            };
            self.items.push(item);
        }
    }

    pub fn values(&self) -> Vec<f64> {
        self.items.iter().map(|p| p.value).collect()
    }

    pub fn any_playing(&self) -> bool {
        self.items.iter().any(|p| p.playing)
    }

    /// Advance the sweeps and show the card (nothing when there are no
    /// parameters).
    pub fn ui(&mut self, ui: &mut Ui, id: &str) -> Change {
        let mut ch = Change::default();
        if self.items.is_empty() {
            return ch;
        }
        let t = tokens(ui.ctx());
        let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
        for p in self.items.iter_mut().filter(|p| p.playing) {
            let span = p.hi - p.lo;
            p.value += p.dir * span * dt / self.seconds.max(0.2);
            if p.value >= p.hi {
                p.value = p.hi;
                p.dir = -1.0;
            } else if p.value <= p.lo {
                p.value = p.lo;
                p.dir = 1.0;
            }
            ch.changed = true;
        }
        if self.any_playing() {
            ui.ctx().request_repaint();
        }
        let seconds = &mut self.seconds;
        let items = &mut self.items;
        card(
            ui,
            id,
            icon::TUNE,
            tr("Parameters", "参数"),
            true,
            |ui| {
                ui.add(egui::DragValue::new(seconds).speed(0.05).range(0.5..=60.0).max_decimals(1).suffix(" s"))
                    .on_hover_text(tr("Time for one sweep when a parameter is playing", "播放时参数扫过一遍所需的时间"));
            },
            |ui| {
                for p in items.iter_mut() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        // `a = 1.25   -5 [——o——] 5   ▶`
                        ui.add_sized([18.0, 20.0], egui::Label::new(RichText::new(&p.name).monospace().size(15.0).color(t.text)));
                        ui.label(RichText::new("=").color(t.faint));
                        let speed = ((p.hi - p.lo).abs() * 0.01).max(1e-3);
                        let v = ui.add_sized([58.0, 20.0], egui::DragValue::new(&mut p.value).speed(speed * 0.5).max_decimals(3)).on_hover_text(tr("Value (drag or type)", "数值（拖动或输入）"));
                        if v.changed() {
                            ch.changed = true;
                            // A value outside the range widens it.
                            p.lo = p.lo.min(p.value);
                            p.hi = p.hi.max(p.value);
                            ch.settled |= !v.dragged();
                        }
                        ch.settled |= v.drag_stopped();
                        ui.add_space(4.0);
                        let range_tip = tr("Slider range (drag or type)", "滑块范围（拖动或输入）");
                        let weak = |ui: &mut Ui, v: &mut f64| {
                            ui.visuals_mut().override_text_color = Some(t.muted);
                            ui.add(egui::DragValue::new(v).speed(speed).max_decimals(2)).on_hover_text(range_tip).changed()
                        };
                        let mut range_changed = ui.scope(|ui| weak(ui, &mut p.lo)).inner;
                        ui.spacing_mut().slider_width = (ui.available_width() - 82.0).max(40.0);
                        // Edit a copy: the slider rounds what it is given.
                        let mut shown = p.value;
                        let r = ui.add(egui::Slider::new(&mut shown, p.lo..=p.hi).show_value(false));
                        if r.changed() {
                            p.value = shown;
                            ch.changed = true;
                        }
                        ch.settled |= r.drag_stopped() || (r.changed() && !r.dragged());
                        range_changed |= ui.scope(|ui| weak(ui, &mut p.hi)).inner;
                        if p.hi <= p.lo {
                            p.hi = p.lo + 1e-3;
                        }
                        if range_changed && p.playing {
                            ch.settled = true;
                        }
                        let glyph = match (icons_available(), p.playing) {
                            (true, true) => icon::PAUSE,
                            (true, false) => icon::PLAY,
                            (false, true) => "||",
                            (false, false) => ">",
                        };
                        let tip = if p.playing { tr("Stop", "停止") } else { tr("Sweep back and forth over the range", "在范围内来回扫动") };
                        let color = if p.playing { t.accent_text } else { t.muted };
                        if ui.add(egui::Button::new(RichText::new(glyph).size(15.0).color(color)).min_size(egui::vec2(26.0, 22.0))).on_hover_text(tip).clicked() {
                            p.playing = !p.playing;
                            ch.settled = true;
                        }
                    });
                }
                caption(ui, tr("Letters that are not variables are parameters. Drag a slider or press play.", "公式中不是变量的字母就是参数。拖动滑块，或点播放让它自动变化。"));
            },
        );
        ch
    }
}
