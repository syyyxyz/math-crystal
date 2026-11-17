//! Windows start-up guard. A GUI program has no console, so a panic or a
//! failed start would otherwise just close silently: show the error in a
//! message box and keep it in %LOCALAPPDATA%\Math-crystal\crash.log. When
//! the graphics back end fails before the window has drawn anything, offer
//! to restart on OpenGL and remember that in backend.txt.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_YESNO, MessageBoxW};

/// Frames after which a failure no longer counts as a start-up failure.
const STARTED_FRAMES: u32 = 3;

fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Math-crystal"))
}

/// Use the back end saved by an earlier "retry with OpenGL", unless
/// WGPU_BACKEND is set. Call before any other thread starts.
pub fn apply_saved_backend() {
    if std::env::var_os("WGPU_BACKEND").is_some() {
        return;
    }
    let Some(saved) = dir().and_then(|d| std::fs::read_to_string(d.join("backend.txt")).ok()) else { return };
    let saved = saved.trim();
    if !saved.is_empty() {
        // SAFETY: called at the top of main, before any other thread exists.
        unsafe { std::env::set_var("WGPU_BACKEND", saved) };
    }
}

/// Report panics (including wgpu validation errors) in a message box.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| report(&info.to_string())));
}

/// Show `error`, write it to crash.log and, for a start-up failure on a
/// back end other than OpenGL, offer to restart on OpenGL.
pub fn report(error: &str) {
    let log = dir().and_then(|d| {
        std::fs::create_dir_all(&d).ok()?;
        let path = d.join("crash.log");
        std::fs::write(&path, error).ok()?;
        Some(path)
    });
    let early = math_crystal::FRAMES.load(Ordering::Relaxed) < STARTED_FRAMES;
    let on_gl = std::env::var("WGPU_BACKEND").is_ok_and(|b| b.trim().eq_ignore_ascii_case("gl"));
    let offer_gl = early && !on_gl;

    let mut shown: String = error.chars().take(1200).collect();
    if shown.len() < error.len() {
        shown.push_str(" …");
    }
    let mut text = if early {
        format!("Math-crystal 无法启动 / could not start.\n\n{shown}\n")
    } else {
        format!("Math-crystal 出错，需要关闭 / hit an error and has to close.\n\n{shown}\n")
    };
    if let Some(path) = &log {
        text += &format!("\n日志 / Log: {}\n", path.display());
    }
    if offer_gl {
        text += "\n这通常是显卡驱动的问题。要改用 OpenGL 重新启动吗？\nThis is usually a graphics driver problem. Restart using OpenGL?";
    } else if early {
        text += "\n请更新显卡驱动后再试。\nPlease update the graphics driver and try again.";
    }

    let style = MB_ICONERROR | MB_SETFOREGROUND | if offer_gl { MB_YESNO } else { MB_OK };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(&text), wide("Math-crystal"));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
    let answer = unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), style) };

    if offer_gl && answer == IDYES {
        if let Some(d) = dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("backend.txt"), "gl\n");
        }
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).env("WGPU_BACKEND", "gl").spawn();
        }
    }
}
