use x11rb::protocol::xproto::Window as XWindow;

use super::tree::Rect;

#[derive(Debug, Clone)]
pub struct Window {
    pub id: XWindow,
    /// Geometry for floating mode (position and size when floating)
    pub floating_geometry: Rect,
    pub mapped: bool,
    pub focused: bool,
    pub floating: bool,
    pub urgent: bool,
    pub workspace: usize,
}

impl Window {
    pub fn new(id: XWindow, workspace: usize) -> Self {
        Self {
            id,
            floating_geometry: Rect::new(0, 0, 640, 480),
            mapped: false,
            focused: false,
            floating: false,
            urgent: false,
            workspace,
        }
    }
}
