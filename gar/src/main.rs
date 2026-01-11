use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use gar::x11::Connection;
use gar::WindowManager;

fn main() {
    // Set up logging
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    tracing::info!("gar {} starting", env!("CARGO_PKG_VERSION"));

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
