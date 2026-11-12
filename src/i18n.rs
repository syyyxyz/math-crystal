//! Interface language (English / Simplified Chinese).
//!
//! Strings are written as pairs at the call site: `tr("Function", "函数")`.
//! The choice is global so that engine messages (parse errors) follow it.

use std::sync::atomic::{AtomicBool, Ordering};

static ZH: AtomicBool = AtomicBool::new(false);

pub fn zh() -> bool {
    ZH.load(Ordering::Relaxed)
}

pub fn set_zh(on: bool) {
    ZH.store(on, Ordering::Relaxed);
}

/// Pick the string for the current language.
pub fn tr<'a>(en: &'a str, zh: &'a str) -> &'a str {
    if self::zh() { zh } else { en }
}

/// Whether the system locale asks for Chinese (used on first start).
pub fn system_prefers_zh() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.starts_with("zh"))
}
