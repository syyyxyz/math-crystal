//! Math-crystal core library (UI-independent parts).
pub mod expr;
pub mod i18n;
pub mod calc;
pub mod render;
pub mod calc_view;
pub mod app;
pub mod ui;
pub mod analysis;
pub mod map_view;
pub mod complex;
pub mod complex_view;
pub mod complex_app;
pub mod spectrum;
pub mod spectrum_app;
pub mod space;
pub mod space_view;
pub mod space_app;
pub mod space_inter;

/// Frames the main window has drawn so far; lets the Windows start-up error
/// report tell a graphics back-end failure (before the first frames) from a
/// later crash.
pub static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
