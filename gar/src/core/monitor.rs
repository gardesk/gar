use x11rb::protocol::randr::Output;

use super::tree::Rect;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    pub output: Output,
    pub geometry: Rect,
    pub primary: bool,
    /// Workspace indices assigned to this monitor
    pub workspaces: Vec<usize>,
    /// Currently active workspace index on this monitor
    pub active_workspace: usize,
}

impl Monitor {
    pub fn new(name: String, output: Output, geometry: Rect) -> Self {
        Self {
            name,
            output,
            geometry,
            primary: false,
            workspaces: Vec::new(),
            active_workspace: 0,
        }
    }
}
