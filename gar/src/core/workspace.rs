use x11rb::protocol::xproto::Window as XWindow;

use super::tree::Node;
use super::Window;

#[derive(Debug)]
pub struct Workspace {
    pub id: usize,
    pub name: String,
    pub tree: Node,
    pub floating: Vec<Window>,
    pub focused: Option<XWindow>,
    pub visible: bool,
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
        }
    }

    pub fn all_windows(&self) -> Vec<XWindow> {
        let mut windows = self.tree.windows();
        windows.extend(self.floating.iter().map(|w| w.id));
        windows
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty() && self.floating.is_empty()
    }
}
