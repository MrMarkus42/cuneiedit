#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod app_icon;
mod compare;
mod find;
mod i18n;
#[cfg(test)]
mod icon_files;
mod instance;
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

fn main() -> eframe::Result {
    // Started elevated by an update on Windows: replace the executable and exit.
    if let Some(code) = cuneiedit_update::apply_from_args() {
        std::process::exit(code);
    }
    cuneiedit_update::cleanup();
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let listener = match instance::start(&files) {
        instance::Start::Forwarded => return Ok(()),
        instance::Start::Main(listener) => Some(listener),
        instance::Start::Alone => None,
    };

    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("CuneiEdit")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(IconData {
                rgba: app_icon::rgba(app::ICON_SIZE),
                width: app::ICON_SIZE,
                height: app::ICON_SIZE,
            })),
        ..Default::default()
    };

    // An own event loop, so the macOS file-open handler is installed before it starts.
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    // winit's default menu quits via `terminate:` without a close request, which would skip
    // the unsaved-changes prompt; CuneiEdit sets its own menu.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS as _;
        builder.with_default_menu(false);
    }
    let event_loop = builder.build()?;
    #[cfg(target_os = "macos")]
    cuneiedit_macos::install_open_handler();

    let mut app = eframe::create_native(
        "CuneiEdit",
        options,
        Box::new(move |cc| {
            let incoming = listener.map(|listener| listener.spawn(cc.egui_ctx.clone()));
            let app = app::CuneiEditApp::new(&cc.egui_ctx, files, incoming);
            Ok(Box::new(app))
        }),
        &event_loop,
    );
    event_loop.run_app(&mut app)?;
    if let Some(path) = app::update::take_relaunch() {
        let _ = cuneiedit_update::relaunch(&path);
    }
    Ok(())
}
