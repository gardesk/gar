use super::tree::Rect;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    pub geometry: Rect,
    pub primary: bool,
    pub workspaces: Vec<usize>,
    pub active_workspace: usize,
}

impl Monitor {
    pub fn new(name: String, geometry: Rect) -> Self {
        Self {
            name,
            geometry,
            primary: false,
            workspaces: Vec::new(),
            active_workspace: 0,
        }
    }
}
