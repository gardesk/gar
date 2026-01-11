use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to connect to X server: {0}")]
    Connect(#[from] x11rb::errors::ConnectError),

    #[error("X11 connection error: {0}")]
    Connection(#[from] x11rb::errors::ConnectionError),

    #[error("X11 reply error: {0}")]
    Reply(#[from] x11rb::errors::ReplyError),

    #[error("X11 reply or ID error: {0}")]
    ReplyOrId(#[from] x11rb::errors::ReplyOrIdError),

    #[error("Another window manager is already running")]
    AnotherWmRunning,

    #[error("No screens available")]
    NoScreens,

    #[error("Window not found: {0}")]
    WindowNotFound(u32),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
