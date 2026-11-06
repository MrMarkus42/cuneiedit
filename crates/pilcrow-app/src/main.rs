#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod app_icon;
mod compare;
mod find;
mod i18n;
mod languages;
mod macros;
mod session;
mod settings;
mod system;
mod themes;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::UserEvent;
use eframe::egui::{IconData, ViewportBuilder};
use winit::event_loop::EventLoop;

/// Size of the window icon in pixels.
const ICON_SIZE: u32 = 256;

fn main() -> eframe::Result {
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("Pilcrow")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(IconData {
                rgba: app_icon::rgba(ICON_SIZE),
                width: ICON_SIZE,
                height: ICON_SIZE,
            })),
        ..Default::default()
    };

    // An own event loop, so the macOS file-open handler is installed before it starts.
    #[cfg_attr(not(target_os = "macos"), expect(unused_mut))]
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    // winit's default menu quits via `terminate:` without a close request, which would skip
    // the unsaved-changes prompt; Pilcrow sets its own menu.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS as _;
        builder.with_default_menu(false);
    }
    let event_loop = builder.build()?;
    #[cfg(target_os = "macos")]
    pilcrow_macos::install_open_handler();

    let mut app = eframe::create_native(
        "Pilcrow",
        options,
        Box::new(|cc| Ok(Box::new(app::PilcrowApp::new(&cc.egui_ctx, files)))),
        &event_loop,
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}
