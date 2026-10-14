//! jterm: a fast, local-first terminal with Warp-style blocks, built on egui and
//! `alacritty_terminal`.

mod app;
mod config;
mod terminal;
mod ui;

use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    init_logging();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("jterm")
            .with_inner_size([1000.0, 650.0]),
        ..Default::default()
    };

    // `eframe::Error` is not `Send + Sync`, so it can't be converted into `anyhow::Error` with `?`.
    eframe::run_native(
        "jterm",
        options,
        Box::new(|cc| Ok(Box::new(app::JtermApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("failed to run the window: {e}"))
}

/// Logs to stderr in debug builds only. Release builds will log to a file in the data
/// dir once config paths exist, so launching from a desktop never needs a console.
/// The level is controlled by `RUST_LOG`. The default silences wgpu-hal driver-probing noise.
fn init_logging() {
    if cfg!(debug_assertions) {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,wgpu_hal=error"));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }
}
