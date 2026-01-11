mod monitor;
mod tree;
mod window;
mod workspace;

pub use monitor::Monitor;
pub use tree::{Direction, Node, Rect, SplitDirection};
pub use window::Window;
pub use workspace::Workspace;

use std::collections::HashMap;
use x11rb::protocol::xproto::Window as XWindow;

use crate::config::Config;
use crate::x11::Connection;
use crate::Result;

pub struct WindowManager {
    pub conn: Connection,
    pub config: Config,
    pub workspaces: Vec<Workspace>,
    pub monitors: Vec<Monitor>,
    pub windows: HashMap<XWindow, Window>,
    pub focused_workspace: usize,
    pub focused_window: Option<XWindow>,
    pub running: bool,
}

impl WindowManager {
    pub fn new(conn: Connection) -> Result<Self> {
        let workspaces = (1..=10)
            .map(|i| Workspace::new(i, i.to_string()))
            .collect();

        Ok(Self {
            conn,
            config: Config::default(),
            workspaces,
            monitors: Vec::new(),
            windows: HashMap::new(),
            focused_workspace: 0,
            focused_window: None,
            running: true,
        })
    }

    pub fn current_workspace(&self) -> &Workspace {
        &self.workspaces[self.focused_workspace]
    }

    pub fn current_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.focused_workspace]
    }

    /// Get the screen rectangle (full usable area).
    pub fn screen_rect(&self) -> Rect {
        Rect::new(0, 0, self.conn.screen_width, self.conn.screen_height)
    }

    /// Check if a window should be managed (not override-redirect, etc.)
    pub fn should_manage(&self, window: XWindow) -> bool {
        // Don't manage the root window
        if window == self.conn.root {
            return false;
        }
        // Already managing?
        if self.windows.contains_key(&window) {
            return false;
        }
        true
    }

    /// Add a window to management.
    pub fn manage_window(&mut self, window: XWindow) {
        if !self.should_manage(window) {
            return;
        }

        tracing::info!("Managing window {}", window);

        // Track the window with current workspace
        let win = Window::new(window, self.focused_workspace);
        self.windows.insert(window, win);

        // Insert into current workspace's tree with smart splitting
        let focused = self.current_workspace().focused;
        let screen = self.screen_rect();
        self.current_workspace_mut()
            .tree
            .insert_with_rect(window, focused, screen);
        self.current_workspace_mut().focused = Some(window);
        self.focused_window = Some(window);
    }

    /// Remove a window from management.
    pub fn unmanage_window(&mut self, window: XWindow) {
        if self.windows.remove(&window).is_some() {
            tracing::info!("Unmanaging window {}", window);

            // Remove from workspace tree
            self.current_workspace_mut().tree.remove(window);

            // Update focus if this was the focused window
            if self.focused_window == Some(window) {
                self.focused_window = self.current_workspace().tree.first_window();
                self.current_workspace_mut().focused = self.focused_window;
            }
        }
    }

    /// Set focus to a window.
    pub fn set_focus(&mut self, window: XWindow) -> Result<()> {
        self.focused_window = Some(window);
        self.current_workspace_mut().focused = Some(window);
        self.conn.set_focus(window)?;
        self.update_borders()?;
        Ok(())
    }

    /// Update border colors for all windows based on focus state.
    pub fn update_borders(&mut self) -> Result<()> {
        let focused = self.focused_window;
        let focused_color = self.config.border_color_focused;
        let unfocused_color = self.config.border_color_unfocused;
        let border_width = self.config.border_width;

        for &window in self.current_workspace().tree.windows().iter() {
            let color = if Some(window) == focused {
                focused_color
            } else {
                unfocused_color
            };
            self.conn.set_border(window, border_width, color)?;
        }
        Ok(())
    }

    /// Apply the current layout to all windows.
    pub fn apply_layout(&mut self) -> Result<()> {
        let border_width = self.config.border_width;

        // Calculate usable area (screen minus borders)
        let screen = self.screen_rect();

        // Get geometries from the tree
        let geometries = self.current_workspace().tree.calculate_geometries(screen);

        // Apply geometries to windows
        for (window, rect) in geometries {
            // Account for border width in geometry
            let adjusted_width = rect.width.saturating_sub(2 * border_width as u16);
            let adjusted_height = rect.height.saturating_sub(2 * border_width as u16);

            self.conn.configure_window(
                window,
                rect.x,
                rect.y,
                adjusted_width.max(1),
                adjusted_height.max(1),
                border_width,
            )?;
        }

        self.update_borders()?;
        self.conn.flush()?;
        Ok(())
    }
}
