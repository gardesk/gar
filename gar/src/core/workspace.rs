use x11rb::protocol::xproto::Window as XWindow;

use super::tree::Node;

#[derive(Debug)]
pub struct Workspace {
    pub id: usize,
    pub name: String,
    pub tree: Node,
    /// Floating window IDs in stacking order (first = bottom, last = top)
    pub floating: Vec<XWindow>,
    pub focused: Option<XWindow>,
    pub visible: bool,
    /// Last monitor this workspace was displayed on (for focus-back behavior)
    pub last_monitor: Option<usize>,
}

impl Workspace {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            tree: Node::empty(),
            floating: Vec::new(),
            focused: None,
            visible: id == 1,
            last_monitor: None,
        }
    }

    pub fn all_windows(&self) -> Vec<XWindow> {
        let mut windows = self.tree.windows();
        windows.extend(self.floating.iter().copied());
        windows
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty() && self.floating.is_empty()
    }

    pub fn has_windows(&self) -> bool {
        !self.is_empty()
    }

    /// Add a window to the floating list (on top)
    pub fn add_floating(&mut self, window: XWindow) {
        if !self.floating.contains(&window) {
            self.floating.push(window);
        }
    }

    /// Remove a window from the floating list
    pub fn remove_floating(&mut self, window: XWindow) {
        self.floating.retain(|&w| w != window);
    }

    /// Raise a floating window to the top of the stacking order
    pub fn raise_floating(&mut self, window: XWindow) {
        if let Some(pos) = self.floating.iter().position(|&w| w == window) {
            self.floating.remove(pos);
            self.floating.push(window);
        }
    }

    /// Check if a window is in the floating list
    pub fn is_floating(&self, window: XWindow) -> bool {
        self.floating.contains(&window)
    }
}
