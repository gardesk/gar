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
    pub fullscreen: bool,
    /// Saved geometry before entering fullscreen (for restore)
    pub pre_fullscreen_geometry: Option<Rect>,
    /// Was the window floating before entering fullscreen?
    pub pre_fullscreen_floating: bool,
    pub urgent: bool,
    pub workspace: usize,
    /// Frame window ID (if title bars are enabled, client is reparented into this)
    pub frame: Option<XWindow>,
    /// Window title (cached from _NET_WM_NAME or WM_NAME)
    pub title: String,
    /// Count of UnmapNotify events to ignore (for intentional unmaps during workspace switch)
    pub ignore_unmap_count: u32,
}

impl Window {
    pub fn new(id: XWindow, workspace: usize) -> Self {
        Self {
            id,
            floating_geometry: Rect::new(0, 0, 640, 480),
            mapped: false,
            focused: false,
            floating: false,
            fullscreen: false,
            pre_fullscreen_geometry: None,
            pre_fullscreen_floating: false,
            urgent: false,
            workspace,
            frame: None,
            title: String::new(),
            ignore_unmap_count: 0,
        }
    }

    /// Get the window to configure (frame if present, otherwise client).
    pub fn outer_window(&self) -> XWindow {
        self.frame.unwrap_or(self.id)
    }
}
