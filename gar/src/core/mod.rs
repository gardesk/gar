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
use crate::ipc::{IpcServer, I3IpcServer};
use crate::x11::Connection;
use crate::x11::events::DragState;
use crate::x11::FrameManager;
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
    /// i3-compatible IPC server for polybar integration
    pub i3_ipc_server: Option<I3IpcServer>,
    /// Timestamp of last pointer warp - used to suppress EnterNotify feedback loop
    pub last_warp: std::time::Instant,
    /// Frame manager for title bars
    pub frames: FrameManager,
    /// Focus history stack - most recently focused windows first (per workspace)
    pub focus_history: Vec<XWindow>,
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

        // Initialize i3-compatible IPC server (optional - graceful failure)
        let i3_ipc_server = match I3IpcServer::new() {
            Ok(server) => Some(server),
            Err(e) => {
                tracing::warn!("Failed to start i3-compatible IPC server: {}", e);
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

        // i3-style: each monitor starts with one workspace (1, 2, 3...)
        // Any workspace can be moved to any monitor dynamically
        for (i, monitor) in monitors.iter_mut().enumerate() {
            monitor.workspaces = vec![i]; // Just track initial workspace
            monitor.active_workspace = i; // Monitor 0 shows ws 0, monitor 1 shows ws 1, etc.
            tracing::debug!("Monitor '{}' starts with workspace {}", monitor.name, i + 1);
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
            i3_ipc_server,
            last_warp: std::time::Instant::now(),
            frames: FrameManager::new(),
            focus_history: Vec::new(),
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

    /// Find which monitor is currently displaying a workspace (i3-style).
    pub fn monitor_for_workspace(&self, workspace_idx: usize) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.active_workspace == workspace_idx)
    }

    /// Find the monitor index currently displaying a workspace (i3-style).
    pub fn monitor_idx_for_workspace(&self, workspace_idx: usize) -> Option<usize> {
        self.monitors.iter().position(|m| m.active_workspace == workspace_idx)
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

        // Try to preserve workspace assignments from old monitors
        // If we have more monitors now, new ones get next available workspaces
        let old_mon_count = self.monitors.len();
        let mut used_workspaces: std::collections::HashSet<usize> = std::collections::HashSet::new();

        for (i, monitor) in new_monitors.iter_mut().enumerate() {
            if i < old_mon_count {
                // Preserve old monitor's workspace
                monitor.active_workspace = self.monitors[i].active_workspace;
                monitor.workspaces = vec![monitor.active_workspace];
                used_workspaces.insert(monitor.active_workspace);
            } else {
                // New monitor - assign first unused workspace
                let first_free = (0..self.workspaces.len())
                    .find(|ws| !used_workspaces.contains(ws))
                    .unwrap_or(0);
                monitor.active_workspace = first_free;
                monitor.workspaces = vec![first_free];
                used_workspaces.insert(first_free);
            }
            tracing::info!("Monitor '{}' showing workspace {}", monitor.name, monitor.active_workspace + 1);
        }

        self.monitors = new_monitors;

        // Ensure focused_monitor is valid
        if self.focused_monitor >= self.monitors.len() {
            self.focused_monitor = 0;
        }

        // Update focused_workspace to match the focused monitor
        self.focused_workspace = self.monitors[self.focused_monitor].active_workspace;

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

        // Update EWMH client lists
        self.update_client_lists();
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

        // Update EWMH client lists
        self.update_client_lists();
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

        // Update EWMH client lists
        self.update_client_lists();
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

        // Update EWMH client lists
        self.update_client_lists();
    }

    /// Create a frame for a window if title bars are enabled.
    /// Returns the frame window ID if created.
    pub fn create_frame_for_window(&mut self, window: XWindow) -> Option<XWindow> {
        if !self.config.titlebar_enabled {
            return None;
        }

        // Get window title for display
        let title = self.conn.get_window_title(window).unwrap_or_default();

        // Create frame with initial geometry (will be updated by apply_layout)
        let screen = self.screen_rect();
        let frame = match self.frames.create_frame(
            &self.conn.conn,
            self.conn.root,
            window,
            screen.x,
            screen.y,
            400, // Initial width, will be adjusted
            300, // Initial height, will be adjusted
            self.config.titlebar_height as u16,
            self.config.border_width as u16,
            self.config.border_color_unfocused,
            self.config.titlebar_color_unfocused,
        ) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to create frame for window {}: {}", window, e);
                return None;
            }
        };

        // Update window state with frame and title
        if let Some(win) = self.windows.get_mut(&window) {
            win.frame = Some(frame);
            win.title = title;
        }

        Some(frame)
    }

    /// Remove a window from management.
    pub fn unmanage_window(&mut self, window: XWindow) {
        if let Some(win) = self.windows.remove(&window) {
            let ws_idx = win.workspace;
            tracing::info!("Unmanaging window {} from workspace {}", window, ws_idx + 1);

            // Destroy frame if it exists
            if win.frame.is_some() {
                if let Err(e) = self.frames.destroy_frame(&self.conn.conn, self.conn.root, window) {
                    tracing::warn!("Failed to destroy frame for window {}: {}", window, e);
                }
            }

            // Remove from the window's actual workspace (not current_workspace!)
            if win.floating {
                self.workspaces[ws_idx].remove_floating(window);
            } else {
                self.workspaces[ws_idx].tree.remove(window);
            }

            // Remove from focus history
            self.focus_history.retain(|&w| w != window);

            // Update focus if this was the focused window
            if self.focused_window == Some(window) {
                // Find next window from focus history that's on this workspace
                let next_from_history = self.focus_history.iter()
                    .find(|&&w| self.windows.get(&w).map(|win| win.workspace == ws_idx).unwrap_or(false))
                    .copied();

                // Fall back to first window in tree or floating list if no history
                self.focused_window = next_from_history
                    .or_else(|| self.workspaces[ws_idx].tree.first_window())
                    .or_else(|| self.workspaces[ws_idx].floating.last().copied());
                self.workspaces[ws_idx].focused = self.focused_window;
            }

            // Update EWMH client lists
            self.update_client_lists();
        }
    }

    /// Update _NET_CLIENT_LIST and _NET_CLIENT_LIST_STACKING on root window.
    pub fn update_client_lists(&self) {
        let windows: Vec<u32> = self.windows.keys().copied().collect();
        if let Err(e) = self.conn.update_client_list(&windows) {
            tracing::warn!("Failed to update client list: {}", e);
        }
        // For stacking order, we use the same list for now (tiling WM doesn't have true stacking)
        // A more sophisticated implementation would order by focus history
        if let Err(e) = self.conn.update_client_list_stacking(&windows) {
            tracing::warn!("Failed to update client list stacking: {}", e);
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
    /// If `warp_pointer` is true, the mouse pointer will be moved to the window center.
    /// Use true for keyboard navigation, false for mouse-initiated focus changes.
    pub fn set_focus(&mut self, window: XWindow, warp_pointer: bool) -> Result<()> {
        self.focused_window = Some(window);
        self.current_workspace_mut().focused = Some(window);

        // Update focus history - move window to front
        self.focus_history.retain(|&w| w != window);
        self.focus_history.insert(0, window);

        // Clear urgency when window receives focus
        if let Some(win) = self.windows.get_mut(&window) {
            if win.urgent {
                tracing::debug!("Clearing urgency for window {} on focus", window);
                win.urgent = false;
            }
        }

        self.conn.set_focus(window)?;
        self.conn.set_active_window(Some(window))?;
        self.update_borders()?;

        // Warp pointer to center of focused window (mouse follows focus)
        if warp_pointer {
            if let Err(e) = self.conn.warp_pointer_to_window(window) {
                tracing::warn!("Failed to warp pointer: {}", e);
            }
            // Record warp time to suppress EnterNotify feedback loop
            self.last_warp = std::time::Instant::now();
        }

        Ok(())
    }

    /// Toggle fullscreen state for a window.
    pub fn toggle_fullscreen(&mut self, window: XWindow) -> Result<()> {
        let win = match self.windows.get_mut(&window) {
            Some(w) => w,
            None => return Ok(()),
        };

        let ws_idx = win.workspace;

        if win.fullscreen {
            // Exit fullscreen - restore previous state
            tracing::info!("Window {} exiting fullscreen", window);

            win.fullscreen = false;

            // Restore previous floating state
            let was_floating = win.pre_fullscreen_floating;
            if was_floating != win.floating {
                if was_floating {
                    // Was floating before - remove from tree, add to floating
                    self.workspaces[ws_idx].tree.remove(window);
                    self.workspaces[ws_idx].add_floating(window);
                } else {
                    // Was tiled before - remove from floating, add to tree
                    self.workspaces[ws_idx].remove_floating(window);
                    let focused = self.workspaces[ws_idx].focused;
                    let screen = self.screen_rect();
                    self.workspaces[ws_idx].tree.insert_with_rect(window, focused, screen);
                }
                if let Some(w) = self.windows.get_mut(&window) {
                    w.floating = was_floating;
                }
            }

            // Clear EWMH fullscreen state
            let _ = self.conn.set_window_state(window, &[]);
        } else {
            // Enter fullscreen
            tracing::info!("Window {} entering fullscreen", window);

            // Save current state
            win.pre_fullscreen_floating = win.floating;
            win.fullscreen = true;

            // Set EWMH fullscreen state
            let _ = self.conn.set_window_state(window, &[self.conn.net_wm_state_fullscreen]);
        }

        // Re-apply layout (fullscreen windows get special treatment in apply_layout)
        self.apply_layout()?;
        self.conn.flush()?;
        Ok(())
    }

    /// Set fullscreen state for a window explicitly (for EWMH client messages).
    pub fn set_fullscreen(&mut self, window: XWindow, fullscreen: bool) -> Result<()> {
        let is_fullscreen = self.windows.get(&window).map(|w| w.fullscreen).unwrap_or(false);
        if is_fullscreen != fullscreen {
            self.toggle_fullscreen(window)?;
        }
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

    /// Update border colors for all visible windows based on focus and urgency state.
    pub fn update_borders(&mut self) -> Result<()> {
        let focused = self.focused_window;
        let focused_color = self.config.border_color_focused;
        let unfocused_color = self.config.border_color_unfocused;
        let urgent_color = self.config.border_color_urgent;
        let border_width = self.config.border_width;

        // Get all visible workspace indices
        let visible_ws: Vec<usize> = self.monitors.iter().map(|m| m.active_workspace).collect();

        // Update borders for all windows on visible workspaces
        for ws_idx in visible_ws {
            for window in self.workspaces[ws_idx].all_windows() {
                // Check if window is urgent (and not focused - focused clears urgency)
                let is_urgent = self.windows.get(&window)
                    .map(|w| w.urgent && Some(window) != focused)
                    .unwrap_or(false);

                let color = if is_urgent {
                    urgent_color
                } else if Some(window) == focused {
                    focused_color
                } else {
                    unfocused_color
                };

                // If window has a frame, set border on the frame instead
                if self.windows.get(&window).and_then(|w| w.frame).is_some() {
                    self.frames.set_frame_border(&self.conn.conn, window, color)?;
                } else {
                    self.conn.set_border(window, border_width, color)?;
                }
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
            // Check for fullscreen windows on this workspace
            let fullscreen_windows: Vec<XWindow> = self.windows.iter()
                .filter(|(_, w)| w.workspace == *ws_idx && w.fullscreen)
                .map(|(id, _)| *id)
                .collect();

            // If there's a fullscreen window, it takes the whole monitor
            if let Some(&fs_window) = fullscreen_windows.first() {
                tracing::debug!(
                    "apply_layout: FULLSCREEN window={} on monitor {:?}",
                    fs_window, screen
                );

                // Configure fullscreen window to cover entire monitor (no gaps, no borders)
                self.conn.configure_window(
                    fs_window,
                    screen.x,
                    screen.y,
                    screen.width,
                    screen.height,
                    0, // No border for fullscreen
                )?;

                // Raise fullscreen window above everything
                let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
                self.conn.conn.configure_window(fs_window, &aux)?;

                // Skip normal layout for this workspace - fullscreen window covers everything
                continue;
            }

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

            // Get titlebar settings
            let titlebar_enabled = self.config.titlebar_enabled;
            let titlebar_height = self.config.titlebar_height as u16;

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

                // Check if window has a frame
                let has_frame = self.windows.get(window).and_then(|w| w.frame).is_some();

                if has_frame && titlebar_enabled {
                    // Configure frame (includes titlebar height)
                    let client_height = final_height.saturating_sub(titlebar_height);
                    self.frames.configure_frame(
                        &self.conn.conn,
                        *window,
                        gapped_x,
                        gapped_y,
                        final_width.max(1),
                        client_height.max(1),
                        titlebar_height,
                        border_width as u16,
                    )?;

                    tracing::debug!(
                        "apply_layout: TILED+FRAME window={} at ({}, {}) size {}x{} (titlebar: {})",
                        window, gapped_x, gapped_y, final_width.max(1), final_height.max(1), titlebar_height
                    );
                } else {
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
            }

            // 2. Configure floating windows and stack them above tiled
            let floating_ids: Vec<XWindow> = ws.floating.clone();

            for window_id in floating_ids {
                // Get the window's floating geometry from our state
                if let Some(win) = self.windows.get(&window_id) {
                    let geom = win.floating_geometry;
                    let adjusted_width = geom.width.saturating_sub(2 * border_width as u16);
                    let adjusted_height = geom.height.saturating_sub(2 * border_width as u16);

                    let has_frame = win.frame.is_some();

                    if has_frame && titlebar_enabled {
                        // Configure frame for floating window
                        let client_height = adjusted_height.saturating_sub(titlebar_height);
                        self.frames.configure_frame(
                            &self.conn.conn,
                            window_id,
                            geom.x,
                            geom.y,
                            adjusted_width.max(1),
                            client_height.max(1),
                            titlebar_height,
                            border_width as u16,
                        )?;

                        // Raise frame to top of stack
                        if let Some(frame) = self.frames.frame_for_client(window_id) {
                            let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
                            self.conn.conn.configure_window(frame, &aux)?;
                        }

                        tracing::debug!(
                            "apply_layout: FLOATING+FRAME window={} at ({}, {}) size {}x{} (raising)",
                            window_id, geom.x, geom.y, adjusted_width.max(1), adjusted_height.max(1)
                        );
                    } else {
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
                    }
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
