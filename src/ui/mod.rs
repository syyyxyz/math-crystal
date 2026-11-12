//! Design system: theme tokens, fonts and custom widgets.
pub mod mathview;
pub mod theme;
pub mod widgets;

/// The app icon as RGBA8 (width, height, pixels).
pub fn icon_rgba() -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../../assets/icon.png")));
    let mut reader = decoder.read_info().expect("icon header");
    let mut buf = vec![0; reader.output_buffer_size().expect("icon size")];
    let info = reader.next_frame(&mut buf).expect("icon data");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}
