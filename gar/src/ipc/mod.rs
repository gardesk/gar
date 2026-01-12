mod protocol;
mod server;

pub use protocol::{Event, Request, Response, WindowInfo, WorkspaceInfo};
pub use server::IpcServer;
