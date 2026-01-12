use std::fs::File;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use gar::x11::Connection;
use gar::WindowManager;

fn main() {
    // Set up logging to both stdout and file
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    let log_file = File::create("/tmp/gar.log").expect("Failed to create log file");

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_writer(log_file).with_ansi(false))
        .with(filter)
        .init();

    tracing::info!("gar {} starting", env!("CARGO_PKG_VERSION"));
    tracing::info!("Logging to /tmp/gar.log");

    if let Err(e) = run() {
        tracing::error!("Fatal error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> gar::Result<()> {
    // Connect to X server
    let conn = Connection::new()?;

    // Become the window manager
    conn.become_wm()?;

    // Create window manager instance
    let mut wm = WindowManager::new(conn)?;

    // Run event loop
    wm.run()?;

    Ok(())
}
