//! PNG export of a canvas: hide the overlays for a frame, take a window
//! screenshot, crop it to the canvas and save it to ~/Pictures/Math-crystal.

use std::sync::{Arc, Mutex};

use egui::{Rect, Ui};

use crate::i18n::tr;
use crate::render::gpu::write_png;

#[derive(Default)]
pub struct Export {
    /// Frames left before the screenshot; overlays are hidden while > 0.
    frames: u8,
    result: Arc<Mutex<Option<String>>>,
    /// A screenshot was requested and has not arrived yet.
    waiting: bool,
}

impl Export {
    pub fn start(&mut self) {
        self.frames = 3;
    }

    /// Whether overlays may be drawn this frame.
    pub fn overlays_visible(&self) -> bool {
        self.frames == 0 && !self.waiting
    }

    /// Drive the export; returns the status message once it is done.
    pub fn step(&mut self, ui: &Ui, rect: Rect, name: &str) -> Option<String> {
        if self.frames > 0 {
            self.frames -= 1;
            if self.frames == 1 {
                self.waiting = true;
                let ppp = ui.ctx().pixels_per_point();
                let slot = self.result.clone();
                let ctx = ui.ctx().clone();
                let name = name.to_string();
                ui.ctx().request_screenshot(move |image| {
                    let msg = save(&image, rect, ppp, &name);
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(msg);
                    }
                    ctx.request_repaint();
                });
            }
            ui.ctx().request_repaint();
        }
        let msg = self.result.lock().ok().and_then(|mut r| r.take());
        if msg.is_some() {
            self.waiting = false;
        }
        msg
    }
}

/// Whether an export status message reports success.
pub fn is_ok(msg: &str) -> bool {
    msg.starts_with("Saved") || msg.starts_with("已保存")
}

fn save(image: &egui::ColorImage, rect: Rect, ppp: f32, name: &str) -> String {
    let crop = image.region(&rect.shrink(1.0), Some(ppp));
    let [w, h] = crop.size;
    let rgba: Vec<u8> = crop.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let dir = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Pictures/Math-crystal");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return format!("{} {e}", tr("Export failed:", "导出失败:"));
    }
    let path = dir.join(format!("{name}-{}.png", timestamp()));
    match write_png(&path, w as u32, h as u32, &rgba) {
        Ok(()) => format!("{} {}", tr("Saved", "已保存"), path.display()),
        Err(e) => format!("{} {e}", tr("Export failed:", "导出失败:")),
    }
}

/// Timestamp YYYYmmdd-HHMMSS (UTC; no timezone database needed).
fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}
