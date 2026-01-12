mod protocol;
mod server;
pub mod i3_compat;
pub mod i3_server;

pub use protocol::{Event, Request, Response, WindowInfo, WorkspaceInfo};
pub use server::IpcServer;
pub use i3_server::{I3IpcServer, WorkspaceInfo as I3WorkspaceInfo, OutputInfo, Rect as I3Rect};
