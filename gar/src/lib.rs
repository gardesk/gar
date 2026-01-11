pub mod config;
pub mod core;
pub mod input;
pub mod ipc;
pub mod x11;

pub use crate::core::WindowManager;
pub use crate::x11::error::Error;

pub type Result<T> = std::result::Result<T, Error>;
