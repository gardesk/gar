mod monitor;
mod tree;
mod window;
mod workspace;

pub use monitor::Monitor;
pub use tree::{Direction, Node, Rect, SplitDirection};
pub use window::Window;
pub use workspace::Workspace;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use x11rb::protocol::xproto::{ConnectionExt, Window as XWindow};

use crate::config::{Config, LuaConfig, LuaState, RuleActions, WindowMatch};
use crate::ipc::IpcServer;
use crate::x11::Connection;
use crate::x11::events::DragState;
use crate::Result;

pub struct WindowManager {
    pub conn: Connection,
    pub config: Config,
    pub lua_config: LuaConfig,
    pub lua_state: Arc<Mutex<LuaState>>,
    pub workspaces: Vec<Workspace>,
    pub monitors: Vec<Monitor>,
    pub windows: HashMap<XWindow, Window>,
    pub focused_workspace: usize,
    pub focused_window: Option<XWindow>,
    pub focused_monitor: usize,
    pub running: bool,
    pub drag_state: Option<DragState>,
    pub ipc_server: Option<IpcServer>,
    /// Timestamp of last pointer warp - used to suppress EnterNotify feedback loop
    pub last_warp: std::time::Instant,
}

impl WindowManager {
    pub fn new(conn: Connection) -> Result<Self> {
        let workspaces: Vec<Workspace> = (1..=10)
            .map(|i| Workspace::new(i, i.to_string()))
            .collect();

        // Initialize Lua config
        let lua_config = LuaConfig::new().map_err(|e| crate::Error::Config(e.to_string()))?;
        let lua_state = lua_config.state();

        // Load configuration
        lua_config
            .load()
            .map_err(|e| crate::Error::Config(e.to_string()))?;

        // Get config values from Lua state
        let config = lua_state.lock().unwrap().config.clone();

        // Initialize IPC server (optional - graceful failure)
        let ipc_server = match IpcServer::new() {
            Ok(server) => Some(server),
            Err(e) => {
                tracing::warn!("Failed to start IPC server: {}", e);
                None
            }
        };

        // Subscribe to RandR events for hotplug
        if let Err(e) = conn.subscribe_randr_events() {
            tracing::warn!("Failed to subscribe to RandR events: {}", e);
        }

        // Detect monitors
        let mut monitors = conn.detect_monitors().unwrap_or_else(|e| {
            tracing::warn!("Failed to detect monitors: {}, using single screen", e);
            vec![Monitor::new(
                "default".to_string(),
                0,
                Rect::new(0, 0, conn.screen_width, conn.screen_height),
            )]
        });

        // Ensure at least one monitor
        if monitors.is_empty() {
            monitors.push(Monitor::new(
                "default".to_string(),
                0,
                Rect::new(0, 0, conn.screen_width, conn.screen_height),
            ));
        }

        // Assign workspaces to monitors
        let ws_count = workspaces.len();
        let mon_count = monitors.len();
        for (i, monitor) in monitors.iter_mut().enumerate() {
            // Distribute workspaces: first monitor gets ws 1-N/M, etc.
            let start = i * ws_count / mon_count;
            let end = (i + 1) * ws_count / mon_count;
            monitor.workspaces = (start..end).collect();
            monitor.active_workspace = start;
            tracing::debug!("Monitor '{}' assigned workspaces {:?}", monitor.name, monitor.workspaces);
        }

        Ok(Self {
            conn,
            config,
            lua_config,
            lua_state,
            workspaces,
            monitors,
            windows: HashMap::new(),
            focused_workspace: 0,
            focused_window: None,
            focused_monitor: 0,
            running: true,
            drag_state: None,
            ipc_server,
            last_warp: std::time::Instant::now(),
        })
    }

    pub fn current_workspace(&self) -> &Workspace {
        &self.workspaces[self.focused_workspace]
    }

    pub fn current_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.focused_workspace]
    }

    pub fn current_monitor(&self) -> &Monitor {
        &self.monitors[self.focused_monitor]
    }

    /// Get the rectangle for the focused monitor.
    pub fn screen_rect(&self) -> Rect {
        self.monitors[self.focused_monitor].geometry
    }

    /// Get the rectangle for a specific workspace's monitor.
    pub fn workspace_rect(&self, workspace_idx: usize) -> Rect {
        self.monitor_for_workspace(workspace_idx)
            .map(|m| m.geometry)
            .unwrap_or_else(|| Rect::new(0, 0, self.conn.screen_width, self.conn.screen_height))
    }

    /// Find which monitor a workspace belongs to.
    pub fn monitor_for_workspace(&self, workspace_idx: usize) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.workspaces.contains(&workspace_idx))
    }

    /// Find the monitor index for a workspace.
    pub fn monitor_idx_for_workspace(&self, workspace_idx: usize) -> Option<usize> {
        self.monitors.iter().position(|m| m.workspaces.contains(&workspace_idx))
    }

    /// Refresh monitors (called on RandR screen change).
    pub fn refresh_monitors(&mut self) -> Result<()> {
        tracing::info!("Refreshing monitor configuration");

        let mut new_monitors = self.conn.detect_monitors().unwrap_or_else(|e| {
            tracing::warn!("Failed to detect monitors: {}, keeping current", e);
            return self.monitors.clone();
        });

        if new_monitors.is_empty() {
            new_monitors.push(Monitor::new(
                "default".to_string(),
                0,
                Rect::new(0, 0, self.conn.screen_width, self.conn.screen_height),
            ));
        }

        // Reassign workspaces to monitors
        let ws_count = self.workspaces.len();
        let mon_count = new_monitors.len();
        for (i, monitor) in new_monitors.iter_mut().enumerate() {
            let start = i * ws_count / mon_count;
            let end = (i + 1) * ws_count / mon_count;
            monitor.workspaces = (start..end).collect();
            monitor.active_workspace = start;
            tracing::info!("Monitor '{}' assigned workspaces {:?}", monitor.name, monitor.workspaces);
        }

        self.monitors = new_monitors;

        // Ensure focused_monitor is valid
        if self.focused_monitor >= self.monitors.len() {
            self.focused_monitor = 0;
        }

        // Ensure focused_workspace is on the focused monitor
        if !self.monitors[self.focused_monitor].workspaces.contains(&self.focused_workspace) {
            self.focused_workspace = self.monitors[self.focused_monitor].active_workspace;
        }

        // Re-apply layout for visible workspaces
        self.apply_layout()?;

        Ok(())
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

        tracing::info!("Managing window {} (tiled)", window);

        // Track the window with current workspace
        let win = Window::new(window, self.focused_workspace);
        self.windows.insert(window, win);

        // Set EWMH _NET_WM_DESKTOP
        let _ = self.conn.set_window_desktop(window, self.focused_workspace as u32);

        // Insert into current workspace's tree with smart splitting
        let focused = self.current_workspace().focused;
        let screen = self.screen_rect();
        self.current_workspace_mut()
            .tree
            .insert_with_rect(window, focused, screen);
        self.current_workspace_mut().focused = Some(window);
        self.focused_window = Some(window);
    }

    /// Add a window to management on a specific workspace.
    pub fn manage_window_on_workspace(&mut self, window: XWindow, workspace_idx: usize) {
        if !self.should_manage(window) {
            return;
        }

        tracing::info!("Managing window {} on workspace {} (tiled)", window, workspace_idx + 1);

        // Track the window
        let win = Window::new(window, workspace_idx);
        self.windows.insert(window, win);

        // Set EWMH _NET_WM_DESKTOP
        let _ = self.conn.set_window_desktop(window, workspace_idx as u32);

        // Insert into target workspace's BSP tree
        let focused = self.workspaces[workspace_idx].focused;
        let screen = self.screen_rect();
        self.workspaces[workspace_idx].tree.insert_with_rect(window, focused, screen);
    }

    /// Add a window to management as a floating window on a specific workspace.
    pub fn manage_window_floating_on_workspace(&mut self, window: XWindow, workspace_idx: usize) {
        if !self.should_manage(window) {
            return;
        }

        tracing::info!("Managing window {} on workspace {} (floating)", window, workspace_idx + 1);

        // Calculate floating geometry
        let screen = self.screen_rect();
        let float_width = (screen.width * 4 / 5).max(400);
        let float_height = (screen.height * 4 / 5).max(300);
        let float_x = screen.x + (screen.width as i16 - float_width as i16) / 2;
        let float_y = screen.y + (screen.height as i16 - float_height as i16) / 2;

        // Track the window with floating state
        let mut win = Window::new(window, workspace_idx);
        win.floating = true;
        win.floating_geometry = Rect::new(float_x, float_y, float_width, float_height);
        self.windows.insert(window, win);

        // Set EWMH _NET_WM_DESKTOP
        let _ = self.conn.set_window_desktop(window, workspace_idx as u32);

        // Add to target workspace's floating list
        self.workspaces[workspace_idx].add_floating(window);
    }

    /// Add a window to management as a floating window.
    pub fn manage_window_floating(&mut self, window: XWindow) {
        if !self.should_manage(window) {
            return;
        }

        tracing::info!("Managing window {} (floating)", window);

        // Calculate centered floating geometry
        let screen = self.screen_rect();
        let float_width = 640.min(screen.width.saturating_sub(40));
        let float_height = 480.min(screen.height.saturating_sub(40));
        let float_x = screen.x + (screen.width as i16 - float_width as i16) / 2;
        let float_y = screen.y + (screen.height as i16 - float_height as i16) / 2;

        // Track the window with floating state
        let mut win = Window::new(window, self.focused_workspace);
        win.floating = true;
        win.floating_geometry = Rect::new(float_x, float_y, float_width, float_height);
        self.windows.insert(window, win);

        // Set EWMH _NET_WM_DESKTOP
        let _ = self.conn.set_window_desktop(window, self.focused_workspace as u32);

        // Add to floating list (on top)
        self.current_workspace_mut().add_floating(window);
        self.current_workspace_mut().focused = Some(window);
        self.focused_window = Some(window);
    }

    /// Remove a window from management.
    pub fn unmanage_window(&mut self, window: XWindow) {
        if let Some(win) = self.windows.remove(&window) {
            let ws_idx = win.workspace;
            tracing::info!("Unmanaging window {} from workspace {}", window, ws_idx + 1);

            // Remove from the window's actual workspace (not current_workspace!)
            if win.floating {
                self.workspaces[ws_idx].remove_floating(window);
            } else {
                self.workspaces[ws_idx].tree.remove(window);
            }

            // Update focus if this was the focused window
            if self.focused_window == Some(window) {
                // Try to focus another window on that workspace
                self.focused_window = self.workspaces[ws_idx].tree.first_window()
                    .or_else(|| self.workspaces[ws_idx].floating.last().copied());
                self.workspaces[ws_idx].focused = self.focused_window;
            }
        }
    }

    /// Check window rules and return actions to apply.
    pub fn check_rules(&self, window: XWindow) -> RuleActions {
        let state = self.lua_state.lock().unwrap();

        // Get window properties
        let (instance, class) = self.conn.get_wm_class(window).unwrap_or_default();
        let title = self.conn.get_window_title(window).unwrap_or_default();

        tracing::debug!("Checking rules for window {}: class={}, instance={}, title={}",
            window, class, instance, title);

        let mut result = RuleActions::default();

        for rule in &state.rules {
            let matches = rule_matches(&rule.match_criteria, &class, &instance, &title);
            if matches {
                tracing::info!("Rule matched for window {}: {:?}", window, rule.actions);
                // Merge actions (later rules override earlier ones)
                if rule.actions.floating.is_some() {
                    result.floating = rule.actions.floating;
                }
                if rule.actions.workspace.is_some() {
                    result.workspace = rule.actions.workspace;
                }
            }
        }

        result
    }

    /// Set focus to a window.
    pub fn set_focus(&mut self, window: XWindow) -> Result<()> {
        self.focused_window = Some(window);
        self.current_workspace_mut().focused = Some(window);
        self.conn.set_focus(window)?;
        self.conn.set_active_window(Some(window))?;
        self.update_borders()?;

        // Warp pointer to center of focused window (mouse follows focus)
        if let Err(e) = self.conn.warp_pointer_to_window(window) {
            tracing::warn!("Failed to warp pointer: {}", e);
        }
        // Record warp time to suppress EnterNotify feedback loop
        self.last_warp = std::time::Instant::now();

        Ok(())
    }

    /// Warp pointer to center of a monitor (for focus without windows)
    pub fn warp_to_monitor(&mut self, monitor_idx: usize) -> Result<()> {
        let geom = self.monitors[monitor_idx].geometry;
        let center_x = geom.x + (geom.width / 2) as i16;
        let center_y = geom.y + (geom.height / 2) as i16;

        self.conn.conn.warp_pointer(
            x11rb::NONE,
            self.conn.root,
            0, 0, 0, 0,
            center_x,
            center_y,
        )?;
        self.last_warp = std::time::Instant::now();
        self.conn.flush()?;
        Ok(())
    }

    /// Update border colors for all visible windows based on focus state.
    pub fn update_borders(&mut self) -> Result<()> {
        let focused = self.focused_window;
        let focused_color = self.config.border_color_focused;
        let unfocused_color = self.config.border_color_unfocused;
        let border_width = self.config.border_width;

        // Get all visible workspace indices
        let visible_ws: Vec<usize> = self.monitors.iter().map(|m| m.active_workspace).collect();

        // Update borders for all windows on visible workspaces
        for ws_idx in visible_ws {
            for window in self.workspaces[ws_idx].all_windows() {
                let color = if Some(window) == focused {
                    focused_color
                } else {
                    unfocused_color
                };
                self.conn.set_border(window, border_width, color)?;
            }
        }
        Ok(())
    }

    /// Apply the current layout to all visible windows across all monitors.
    /// Each monitor displays its active_workspace.
    /// Stacking order: tiled windows at bottom, floating windows on top (in list order).
    pub fn apply_layout(&mut self) -> Result<()> {
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt, StackMode};

        let border_width = self.config.border_width;
        let gap_outer = self.config.gap_outer as i16;
        let gap_inner = self.config.gap_inner as i16;
        let half_gap = gap_inner / 2;

        // Collect visible workspaces (one per monitor)
        let visible_workspaces: Vec<(usize, Rect)> = self.monitors
            .iter()
            .map(|m| (m.active_workspace, m.geometry))
            .collect();

        // Layout each monitor's active workspace
        for (ws_idx, screen) in &visible_workspaces {
            let work_area = Rect::new(
                screen.x + gap_outer,
                screen.y + gap_outer,
                screen.width.saturating_sub(2 * gap_outer as u16),
                screen.height.saturating_sub(2 * gap_outer as u16),
            );

            let ws = &self.workspaces[*ws_idx];
            tracing::debug!(
                "apply_layout: ws={} screen={:?}, work_area={:?}, tiled={}, floating={}",
                ws_idx + 1, screen, work_area,
                ws.tree.window_count(), ws.floating.len()
            );

            // 1. Configure tiled windows from the BSP tree
            let geometries = ws.tree.calculate_geometries(work_area);
            for (window, rect) in &geometries {
                // Apply inner gap: shrink each window by half_gap on each side
                let gapped_x = rect.x + half_gap;
                let gapped_y = rect.y + half_gap;
                let gapped_width = rect.width.saturating_sub(gap_inner as u16);
                let gapped_height = rect.height.saturating_sub(gap_inner as u16);

                // Account for border width
                let final_width = gapped_width.saturating_sub(2 * border_width as u16);
                let final_height = gapped_height.saturating_sub(2 * border_width as u16);

                tracing::debug!(
                    "apply_layout: TILED window={} at ({}, {}) size {}x{}",
                    window, gapped_x, gapped_y, final_width.max(1), final_height.max(1)
                );

                self.conn.configure_window(
                    *window,
                    gapped_x,
                    gapped_y,
                    final_width.max(1),
                    final_height.max(1),
                    border_width,
                )?;
            }

            // 2. Configure floating windows and stack them above tiled
            let floating_ids: Vec<XWindow> = ws.floating.clone();

            for window_id in floating_ids {
                // Get the window's floating geometry from our state
                if let Some(win) = self.windows.get(&window_id) {
                    let geom = win.floating_geometry;
                    let adjusted_width = geom.width.saturating_sub(2 * border_width as u16);
                    let adjusted_height = geom.height.saturating_sub(2 * border_width as u16);

                    tracing::debug!(
                        "apply_layout: FLOATING window={} at ({}, {}) size {}x{} (raising)",
                        window_id, geom.x, geom.y, adjusted_width.max(1), adjusted_height.max(1)
                    );

                    // Configure geometry
                    self.conn.configure_window(
                        window_id,
                        geom.x,
                        geom.y,
                        adjusted_width.max(1),
                        adjusted_height.max(1),
                        border_width,
                    )?;

                    // Raise to top of stack (each subsequent window goes above the previous)
                    let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
                    self.conn.conn.configure_window(window_id, &aux)?;
                } else {
                    tracing::warn!("apply_layout: floating window {} not in windows map!", window_id);
                }
            }
        }

        self.update_borders()?;
        self.conn.flush()?;
        Ok(())
    }

    /// Check if a workspace is currently visible (active on any monitor).
    pub fn is_workspace_visible(&self, ws_idx: usize) -> bool {
        self.monitors.iter().any(|m| m.active_workspace == ws_idx)
    }

    /// Get all currently visible workspace indices.
    pub fn visible_workspaces(&self) -> Vec<usize> {
        self.monitors.iter().map(|m| m.active_workspace).collect()
    }
}

/// Check if window properties match rule criteria (case-insensitive substring match).
fn rule_matches(criteria: &WindowMatch, class: &str, instance: &str, title: &str) -> bool {
    let class_lower = class.to_lowercase();
    let instance_lower = instance.to_lowercase();
    let title_lower = title.to_lowercase();

    // All specified criteria must match
    if let Some(ref c) = criteria.class {
        let c_lower: String = c.to_lowercase();
        if !class_lower.contains(&c_lower) {
            return false;
        }
    }
    if let Some(ref i) = criteria.instance {
        let i_lower: String = i.to_lowercase();
        if !instance_lower.contains(&i_lower) {
            return false;
        }
    }
    if let Some(ref t) = criteria.title {
        let t_lower: String = t.to_lowercase();
        if !title_lower.contains(&t_lower) {
            return false;
        }
    }

    // At least one criterion must be specified
    criteria.class.is_some() || criteria.instance.is_some() || criteria.title.is_some()
}
