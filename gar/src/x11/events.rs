use std::process::Command;

use x11rb::connection::Connection as X11Connection;

/// Reap any zombie child processes to prevent accumulation.
/// Called periodically from the event loop.
fn reap_zombies() {
    unsafe {
        // WNOHANG = 1, reap any child without blocking
        while libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) > 0 {}
    }
}

/// Signal systemd that the graphical session has started.
/// This allows user services bound to graphical-session.target to start.
fn start_graphical_session() {
    // Import DISPLAY so user services can connect to X
    if let Ok(display_val) = std::env::var("DISPLAY") {
        match Command::new("systemctl")
            .args(["--user", "import-environment", "DISPLAY", "XAUTHORITY"])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    tracing::info!("Imported DISPLAY={} to systemd user session", display_val);
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Failed to import DISPLAY: {}", stderr);
                }
            }
            Err(e) => tracing::error!("Failed to run systemctl import-environment: {}", e),
        }
    } else {
        tracing::warn!("DISPLAY not set, skipping systemd import");
    }

    // Start graphical-session.target
    match Command::new("systemctl")
        .args(["--user", "start", "graphical-session.target"])
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                tracing::info!("Started graphical-session.target");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!("Failed to start graphical-session.target: {}", stderr);
            }
        }
        Err(e) => {
            tracing::error!("Failed to run systemctl start graphical-session.target: {}", e);
        }
    }
}

/// Signal systemd that the graphical session has ended.
/// This stops user services bound to graphical-session.target.
fn stop_graphical_session() {
    match Command::new("systemctl")
        .args(["--user", "stop", "graphical-session.target"])
        .status()
    {
        Ok(status) if status.success() => {
            tracing::info!("Stopped graphical-session.target");
        }
        Ok(_) => {
            tracing::debug!("graphical-session.target was not running");
        }
        Err(e) => {
            tracing::warn!("Failed to stop graphical-session.target: {}", e);
        }
    }
}

/// Spawn garbar as a child process.
/// Returns the child process handle if successful.
fn spawn_garbar() -> Option<std::process::Child> {
    tracing::info!("Spawning garbar...");

    // Try to find garbar in PATH or common locations
    let garbar_cmd = which_garbar().unwrap_or_else(|| "garbar".to_string());

    // Determine i3 IPC socket path (same as gar's i3_server.rs)
    let i3sock = std::env::var("XDG_RUNTIME_DIR")
        .map(|dir| format!("{}/gar-i3.sock", dir))
        .unwrap_or_else(|_| "/tmp/gar-i3.sock".to_string());

    match Command::new(&garbar_cmd)
        .arg("daemon")
        .env("I3SOCK", &i3sock)
        .spawn()
    {
        Ok(child) => {
            tracing::info!("garbar started (PID {}), I3SOCK={}", child.id(), i3sock);
            Some(child)
        }
        Err(e) => {
            tracing::error!("Failed to spawn garbar: {}", e);
            tracing::info!("Hint: Ensure garbar is installed and in PATH, or use 'cargo install --path garbar'");
            None
        }
    }
}

/// Find garbar executable
fn which_garbar() -> Option<String> {
    // Check if garbar is in PATH
    if Command::new("which")
        .arg("garbar")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some("garbar".to_string());
    }

    // Check common cargo install location
    if let Ok(home) = std::env::var("HOME") {
        let cargo_bin = format!("{}/.cargo/bin/garbar", home);
        if std::path::Path::new(&cargo_bin).exists() {
            return Some(cargo_bin);
        }
    }

    // Check /usr/local/bin
    if std::path::Path::new("/usr/local/bin/garbar").exists() {
        return Some("/usr/local/bin/garbar".to_string());
    }

    None
}

/// Stop garbar gracefully by sending SIGTERM.
fn stop_garbar(child: &mut std::process::Child) {
    tracing::info!("Stopping garbar (PID {})...", child.id());

    // Send SIGTERM for graceful shutdown
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }

    // Wait briefly for it to exit
    match child.try_wait() {
        Ok(Some(status)) => {
            tracing::info!("garbar exited with status: {}", status);
        }
        Ok(None) => {
            // Give it a moment to shut down
            std::thread::sleep(std::time::Duration::from_millis(100));
            match child.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!("garbar exited with status: {}", status);
                }
                Ok(None) => {
                    // Force kill if still running
                    tracing::warn!("garbar did not exit gracefully, sending SIGKILL");
                    let _ = child.kill();
                }
                Err(e) => {
                    tracing::warn!("Error waiting for garbar: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Error checking garbar status: {}", e);
        }
    }
}

/// Signal garbar to reload its configuration (SIGHUP).
fn reload_garbar(child: &std::process::Child) {
    tracing::info!("Signaling garbar to reload (PID {})...", child.id());
    unsafe {
        libc::kill(child.id() as i32, libc::SIGHUP);
    }
}
use x11rb::protocol::xproto::{
    ButtonPressEvent, ButtonReleaseEvent, ClientMessageEvent, ConfigureRequestEvent,
    ConfigureWindowAux, ConnectionExt, DestroyNotifyEvent, EnterNotifyEvent, EventMask,
    ExposeEvent, KeyPressEvent, MapRequestEvent, ModMask, MotionNotifyEvent, NotifyMode,
    PropertyNotifyEvent, StackMode, UnmapNotifyEvent,
};
use x11rb::protocol::Event;

use crate::config::Action;
use crate::core::{Direction, Node, Rect, WindowManager};
use crate::Result;

/// State for mouse drag operations
#[derive(Debug, Clone)]
pub enum DragState {
    Move {
        window: u32,
        start_x: i16,
        start_y: i16,
        start_geometry: Rect,
    },
    Resize {
        window: u32,
        start_x: i16,
        start_y: i16,
        start_geometry: Rect,
        edge: ResizeEdge,
    },
    /// Dragging a tiled window to swap with another
    TiledSwap {
        window: u32,
        /// Grab point offset from window origin (for smooth dragging)
        grab_offset_x: i16,
        grab_offset_y: i16,
        /// Original tree state for reverting if cancelled
        original_tree: Node,
        /// Cached geometries of all tiled windows (updated after swaps)
        tiled_geometries: Vec<(u32, Rect)>,
        /// Currently hovered swap target (for visual feedback)
        hover_target: Option<u32>,
        /// Original workspace index
        workspace: usize,
    },
    /// Dragging on the gap between tiled windows to resize
    TiledResize {
        /// Direction of resize (Right = vertical split, Down = horizontal split)
        direction: Direction,
        /// Starting cursor position (x for horizontal, y for vertical)
        start_pos: i16,
        /// Starting ratio of the split
        start_ratio: f32,
        /// Window whose split we're adjusting
        window: u32,
        /// Total size of the split container (for pixel-to-ratio conversion)
        container_size: u16,
        /// Workspace index
        workspace: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    None,
}

impl WindowManager {
    /// Set up initial keybinds and grabs from Lua config.
    pub fn setup_grabs(&mut self) -> Result<()> {
        let state = self.lua_state.lock().unwrap();
        for keybind in &state.keybinds {
            if let Some(keycode) = self.conn.keycode_from_keysym(keybind.keysym) {
                self.conn.grab_key(keybind.modifiers, keycode)?;
                tracing::debug!(
                    "Grabbed {:?}+keycode {} for {:?}",
                    keybind.modifiers,
                    keycode,
                    keybind.action
                );
            }
        }

        // Grab Alt+Button1/Button3 on root for floating window move/resize
        self.conn.grab_mod_buttons()?;

        self.conn.flush()?;
        tracing::info!("{} keybinds registered", state.keybinds.len());
        Ok(())
    }

    /// Set up EWMH workspace hints for status bar integration.
    pub fn setup_ewmh_hints(&self) -> Result<()> {
        // Advertise supported EWMH atoms
        self.conn.set_ewmh_supported()?;

        // Create WM check window for EWMH identification
        self.conn.setup_wm_check()?;

        // Initialize empty client lists
        self.conn.update_client_list(&[])?;
        self.conn.update_client_list_stacking(&[])?;

        // Set number of desktops
        let num_desktops = self.workspaces.len() as u32;
        self.conn.set_number_of_desktops(num_desktops)?;

        // Set desktop names
        let names: Vec<String> = self.workspaces.iter().map(|ws| ws.name.clone()).collect();
        self.conn.set_desktop_names(&names)?;

        // Set current desktop
        self.conn.set_current_desktop(self.focused_workspace as u32)?;

        // Set active window (none at startup)
        self.conn.set_active_window(None)?;

        self.conn.flush()?;
        tracing::info!("EWMH workspace hints initialized: {} desktops", num_desktops);
        Ok(())
    }

    /// Adopt any existing windows that were already mapped before we started.
    pub fn adopt_existing_windows(&mut self) -> Result<()> {
        use x11rb::protocol::xproto::MapState;

        // Query children of root window
        let reply = self.conn.conn.query_tree(self.conn.root)?.reply()?;
        let mut adopted = 0;

        for &window in &reply.children {
            // Get window attributes to check if it's mapped and not override-redirect
            let attrs = match self.conn.conn.get_window_attributes(window)?.reply() {
                Ok(a) => a,
                Err(_) => continue, // Window may have been destroyed
            };

            // Skip override-redirect windows (menus, tooltips, etc.)
            if attrs.override_redirect {
                continue;
            }

            // Only adopt currently mapped windows
            if attrs.map_state != MapState::VIEWABLE {
                continue;
            }

            // Skip if we already manage this window
            if self.windows.contains_key(&window) {
                continue;
            }

            // Check window rules and EWMH hints
            let rule_actions = self.check_rules(window);
            let should_float = rule_actions.floating.unwrap_or_else(|| self.conn.should_float(window));

            // Subscribe to events on the window
            // All windows get POINTER_MOTION for edge resize cursor feedback
            let events = EventMask::ENTER_WINDOW
                | EventMask::FOCUS_CHANGE
                | EventMask::PROPERTY_CHANGE
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::POINTER_MOTION;
            self.conn.select_input(window, events)?;

            // Grab button for click-to-focus
            self.conn.grab_button(window)?;

            // Manage the window
            if should_float {
                self.manage_window_floating(window);
            } else {
                self.manage_window(window);
            }
            adopted += 1;
        }

        if adopted > 0 {
            self.apply_layout()?;
            // Focus the first window
            if let Some(window) = self.focused_window {
                self.set_focus(window, true)?;
            }
            tracing::info!("Adopted {} existing windows", adopted);
        }

        Ok(())
    }

    pub fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::MapRequest(e) => self.handle_map_request(e)?,
            Event::ConfigureRequest(e) => self.handle_configure_request(e)?,
            Event::UnmapNotify(e) => self.handle_unmap_notify(e)?,
            Event::DestroyNotify(e) => self.handle_destroy_notify(e)?,
            Event::ButtonPress(e) => self.handle_button_press(e)?,
            Event::ButtonRelease(e) => self.handle_button_release(e)?,
            Event::MotionNotify(e) => self.handle_motion_notify(e)?,
            Event::KeyPress(e) => self.handle_key_press(e)?,
            Event::EnterNotify(e) => {
                self.handle_enter_notify(e)?;
            }
            Event::RandrScreenChangeNotify(_) => {
                tracing::info!("RandR screen change detected, refreshing monitors");
                self.refresh_monitors()?;
                self.broadcast_i3_output_event();
            }
            Event::RandrNotify(_) => {
                tracing::info!("RandR notify event, refreshing monitors");
                self.refresh_monitors()?;
                self.broadcast_i3_output_event();
            }
            Event::ClientMessage(e) => {
                self.handle_client_message(e)?;
            }
            Event::PropertyNotify(e) => {
                self.handle_property_notify(e)?;
            }
            Event::Expose(e) => {
                self.handle_expose(e)?;
            }
            _ => {
                tracing::trace!("Unhandled event: {:?}", event);
            }
        }
        Ok(())
    }

    fn handle_map_request(&mut self, event: MapRequestEvent) -> Result<()> {
        let window = event.window;
        tracing::debug!("MapRequest for window {}", window);

        // Check for dock/desktop windows (polybar, etc.) - don't manage, just map
        if self.conn.should_ignore(window) {
            tracing::info!("Window {} is dock/desktop, mapping without managing", window);
            self.conn.map_window(window)?;

            // Read struts from dock windows (reserved screen areas)
            if let Some(strut) = self.conn.get_strut(window) {
                tracing::info!(
                    "Dock window {} has strut: left={}, right={}, top={}, bottom={}",
                    window, strut.left, strut.right, strut.top, strut.bottom
                );
                self.dock_struts.insert(window, strut);
                // Re-apply layout to respect new strut
                self.apply_layout()?;
            }

            self.conn.flush()?;
            return Ok(());
        }

        // Check if we should manage this window
        if !self.should_manage(window) {
            // Just map it without managing
            self.conn.map_window(window)?;
            self.conn.flush()?;
            return Ok(());
        }

        // Check window rules first
        let rule_actions = self.check_rules(window);

        // Determine target workspace (rule or current)
        let target_workspace = rule_actions.workspace.unwrap_or(self.focused_workspace + 1);
        let target_idx = target_workspace.saturating_sub(1).min(self.workspaces.len() - 1);

        // Determine if window should float (rule > ICCCM/EWMH hints)
        let should_float = rule_actions.floating.unwrap_or_else(|| self.conn.should_float(window));

        // Subscribe to events on the window
        // All windows get POINTER_MOTION for edge resize cursor feedback
        let events = EventMask::ENTER_WINDOW
            | EventMask::FOCUS_CHANGE
            | EventMask::PROPERTY_CHANGE
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::POINTER_MOTION;
        self.conn.select_input(window, events)?;

        // Grab button for click-to-focus
        self.conn.grab_button(window)?;

        // Manage window on target workspace
        if target_idx != self.focused_workspace {
            // Window goes to a different workspace
            if should_float {
                self.manage_window_floating_on_workspace(window, target_idx);
            } else {
                self.manage_window_on_workspace(window, target_idx);
            }
            // Create frame if title bars enabled
            self.create_frame_for_window(window);
            // Don't map - it's on another workspace
        } else {
            // Window goes to current workspace
            if should_float {
                self.manage_window_floating(window);
            } else {
                self.manage_window(window);
            }
            // Create frame if title bars enabled
            let frame = self.create_frame_for_window(window);

            // Map the window (and frame if present)
            if frame.is_some() {
                self.frames.map_frame(&self.conn.conn, window)?;
            }
            self.conn.map_window(window)?;
        }

        // Apply layout to all windows
        self.apply_layout()?;

        // Focus the new window (only if on current workspace)
        if target_idx == self.focused_workspace {
            self.set_focus(window, true)?;
        }

        Ok(())
    }

    fn handle_configure_request(&mut self, event: ConfigureRequestEvent) -> Result<()> {
        tracing::trace!("ConfigureRequest for window {}", event.window);

        // If we're not managing this window, pass through the request
        if !self.windows.contains_key(&event.window) {
            let aux = ConfigureWindowAux::from_configure_request(&event);
            self.conn.conn.configure_window(event.window, &aux)?;
            self.conn.flush()?;
        }
        // If we are managing it, we control its geometry via apply_layout()

        Ok(())
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) -> Result<()> {
        let window = event.window;
        tracing::debug!("UnmapNotify for window {} (event on {})", window, event.event);

        // Only handle SubstructureNotify events (event.event == root)
        // Ignore StructureNotify events sent directly to the window (event.event == window)
        // This prevents double-processing since we get both types of events
        if event.event != self.conn.root {
            tracing::debug!("Ignoring UnmapNotify (not from root, likely StructureNotify)");
            return Ok(());
        }

        // Check if this was a dock window with struts
        if self.dock_struts.remove(&window).is_some() {
            tracing::info!("Dock window {} unmapped, removing strut", window);
            self.apply_layout()?;
            self.conn.flush()?;
            return Ok(());
        }

        // Check if we intentionally unmapped this window (workspace switch, move, etc.)
        // If so, decrement the counter and ignore this UnmapNotify
        if let Some(win) = self.windows.get_mut(&window) {
            if win.ignore_unmap_count > 0 {
                win.ignore_unmap_count -= 1;
                tracing::debug!("Ignoring UnmapNotify for window {} (intentional unmap, count now {})",
                    window, win.ignore_unmap_count);
                return Ok(());
            }
        }

        // Check if this window is on a visible workspace (any monitor's active workspace)
        let is_visible = self.windows.get(&window)
            .map(|w| self.is_workspace_visible(w.workspace))
            .unwrap_or(false);

        // Only unmanage if the window was visible - windows on hidden workspaces
        // are unmapped intentionally by us during workspace switching
        if is_visible {
            self.unmanage_window(window);

            // Clear the entire root window to remove any leftover pixels
            self.conn.clear_root_area(0, 0, self.conn.screen_width, self.conn.screen_height)?;

            self.apply_layout()?;

            // Focus next window or warp to monitor if none left
            if let Some(win) = self.focused_window {
                self.set_focus(win, true)?;
            } else {
                // No windows left, warp to current monitor center
                self.warp_to_monitor(self.focused_monitor)?;
            }

            self.conn.flush()?;
        }

        Ok(())
    }

    fn handle_destroy_notify(&mut self, event: DestroyNotifyEvent) -> Result<()> {
        tracing::debug!("DestroyNotify for window {}", event.window);

        // Clean up drag state if the destroyed window was involved in a drag
        if let Some(ref drag) = self.drag_state {
            let drag_window = match drag {
                DragState::Move { window, .. } |
                DragState::Resize { window, .. } |
                DragState::TiledSwap { window, .. } |
                DragState::TiledResize { window, .. } => *window,
            };
            if drag_window == event.window {
                self.drag_state = None;
                let _ = self.conn.ungrab_pointer();
            }
        }

        // Check if this was a dock window with struts
        if self.dock_struts.remove(&event.window).is_some() {
            tracing::info!("Dock window {} destroyed, removing strut", event.window);
        }

        // Remove from management
        self.unmanage_window(event.window);

        // Clear the entire root window to remove any leftover pixels
        // This is needed because X11 without a compositor doesn't automatically repaint
        self.conn.clear_root_area(0, 0, self.conn.screen_width, self.conn.screen_height)?;

        // Re-apply layout
        self.apply_layout()?;

        // Focus next window or warp to monitor if none left
        if let Some(window) = self.focused_window {
            self.set_focus(window, true)?;
        } else {
            // No windows left, warp to current monitor center
            self.warp_to_monitor(self.focused_monitor)?;
        }

        self.conn.flush()?;
        Ok(())
    }

    fn handle_button_press(&mut self, event: ButtonPressEvent) -> Result<()> {
        let window = event.event;
        let child = event.child;
        tracing::debug!("ButtonPress on window {}, child {}, button {}", window, child, event.detail);

        // If we're already in a drag, ignore additional button presses
        if self.drag_state.is_some() {
            tracing::debug!("Already in drag state, ignoring ButtonPress");
            return Ok(());
        }

        // Check for mod+click on floating windows (move/resize)
        // Support both Alt (MOD1) and Super (MOD4) as the modifier
        let has_mod = event.state.contains(x11rb::protocol::xproto::KeyButMask::MOD1)
            || event.state.contains(x11rb::protocol::xproto::KeyButMask::MOD4);

        // For Alt+click from root grab, use child window (the window under cursor)
        let target = if window == self.conn.root && child != 0 {
            child
        } else {
            window
        };

        if has_mod && self.is_floating(target) {
            let geometry = self.get_floating_geometry(target);

            if event.detail == 1 {
                // Mod+Button1 = Move
                tracing::debug!("Starting move for floating window {}", target);
                self.drag_state = Some(DragState::Move {
                    window: target,
                    start_x: event.root_x,
                    start_y: event.root_y,
                    start_geometry: geometry,
                });
                // Clear window cursor to prevent conflict with grab cursor
                self.conn.clear_window_cursor(target)?;
                // Grab pointer for motion events (use fleur/move cursor)
                self.conn.grab_pointer(Some(self.conn.cursor_move))?;
                return Ok(());
            } else if event.detail == 3 {
                // Mod+Button3 = Resize (quadrant-based edge detection)
                let edge = determine_resize_edge_quadrant(&geometry, event.root_x, event.root_y);
                tracing::debug!("Starting resize for floating window {}, edge {:?}", target, edge);
                self.drag_state = Some(DragState::Resize {
                    window: target,
                    start_x: event.root_x,
                    start_y: event.root_y,
                    start_geometry: geometry,
                    edge,
                });
                // Clear window cursor to prevent conflict with grab cursor
                self.conn.clear_window_cursor(target)?;
                let cursor = self.cursor_for_edge(edge);
                self.conn.grab_pointer(Some(cursor))?;
                return Ok(());
            }
        }

        // Mod+Button1 on TILED window = swap drag
        if has_mod && !self.is_floating(target) && self.windows.contains_key(&target) && event.detail == 1 {
            if self.current_workspace().tree.contains(target) {
                let screen = self.screen_rect();
                let tiled_geometries = self.current_workspace()
                    .tree
                    .calculate_geometries(screen);
                let original_tree = self.current_workspace().tree.clone();

                // Calculate grab offset from window origin for smooth dragging
                let window_geom = tiled_geometries.iter()
                    .find(|(w, _)| *w == target)
                    .map(|(_, r)| *r)
                    .unwrap_or_default();
                let grab_offset_x = event.root_x - window_geom.x;
                let grab_offset_y = event.root_y - window_geom.y;

                tracing::debug!("Starting tiled swap drag for window {}, offset=({},{})",
                    target, grab_offset_x, grab_offset_y);
                self.drag_state = Some(DragState::TiledSwap {
                    window: target,
                    grab_offset_x,
                    grab_offset_y,
                    original_tree,
                    tiled_geometries,
                    hover_target: None,
                    workspace: self.focused_workspace,
                });
                // Raise the dragged window above others
                self.conn.raise_window(target)?;
                self.conn.grab_pointer(Some(self.conn.cursor_move))?;
                return Ok(());
            }
        }

        // Check for edge resize on TILED windows (click on gap between windows, no mod key)
        if !has_mod && event.detail == 1 {
            let work_area = self.work_area();
            let geometries = self.current_workspace().tree.calculate_geometries(work_area);

            if let Some((window, direction, container_size)) =
                self.find_tiled_edge(event.root_x, event.root_y, &geometries)
            {
                // Get current ratio from tree
                let start_ratio = self
                    .current_workspace()
                    .tree
                    .get_split_ratio(window, direction)
                    .unwrap_or(0.5);

                let start_pos = match direction {
                    Direction::Left | Direction::Right => event.root_x,
                    Direction::Up | Direction::Down => event.root_y,
                };

                tracing::debug!(
                    "Starting tiled edge resize: window={}, direction={:?}, ratio={}, container={}",
                    window, direction, start_ratio, container_size
                );

                self.drag_state = Some(DragState::TiledResize {
                    direction,
                    start_pos,
                    start_ratio,
                    window,
                    container_size,
                    workspace: self.focused_workspace,
                });

                let cursor = match direction {
                    Direction::Left | Direction::Right => self.conn.cursor_h_double,
                    Direction::Up | Direction::Down => self.conn.cursor_v_double,
                };
                self.conn.grab_pointer(Some(cursor))?;
                return Ok(());
            }
        }

        // Check for edge resize on floating windows (click on edge without mod key)
        let is_floating_win = self.is_floating(window);
        tracing::debug!(
            "Edge resize check: window={}, is_floating={}, button={}, pos=({},{})",
            window, is_floating_win, event.detail, event.root_x, event.root_y
        );

        if !has_mod && is_floating_win && event.detail == 1 {
            let geometry = self.get_floating_geometry(window);
            let edge = determine_resize_edge(&geometry, event.root_x, event.root_y);
            tracing::debug!(
                "Edge detection: geometry=({},{} {}x{}), edge={:?}",
                geometry.x, geometry.y, geometry.width, geometry.height, edge
            );
            if edge != ResizeEdge::None {
                tracing::info!("Starting edge resize for floating window {}, edge {:?}", window, edge);
                self.drag_state = Some(DragState::Resize {
                    window,
                    start_x: event.root_x,
                    start_y: event.root_y,
                    start_geometry: geometry.clone(),
                    edge,
                });
                tracing::debug!("Grabbing pointer for resize, geometry={:?}", geometry);
                // Clear window cursor to prevent conflict with grab cursor
                self.conn.clear_window_cursor(window)?;
                let cursor = self.cursor_for_edge(edge);
                self.conn.grab_pointer(Some(cursor))?;
                self.conn.flush()?;
                return Ok(());
            }
            // Not on edge - if this is a focused floating window, replay the click
            if self.focused_window == Some(window) {
                self.conn.conn.allow_events(
                    x11rb::protocol::xproto::Allow::REPLAY_POINTER,
                    x11rb::CURRENT_TIME,
                )?;
                self.conn.flush()?;
                return Ok(());
            }
        }

        // Only handle if we manage this window
        if !self.windows.contains_key(&window) {
            return Ok(());
        }

        // Focus the clicked window
        if self.focused_window != Some(window) {
            // Regrab button on old focused window for click-to-focus
            if let Some(old) = self.focused_window {
                self.conn.grab_button(old)?;
            }

            // Set focus
            self.set_focus(window, false)?;

            // Ungrab buttons so clicks pass through to the application
            // Edge resize detection uses POINTER_MOTION events, not button grabs
            self.conn.ungrab_button(window)?;

            // Raise floating windows on focus
            if self.is_floating(window) {
                self.raise_window(window)?;
            }

            // Replay the click so the application receives it
            self.conn.conn.allow_events(
                x11rb::protocol::xproto::Allow::REPLAY_POINTER,
                x11rb::CURRENT_TIME,
            )?;
        }

        self.conn.flush()?;
        Ok(())
    }

    fn handle_button_release(&mut self, event: ButtonReleaseEvent) -> Result<()> {
        tracing::debug!("ButtonRelease: button={}, window={}, in_drag={}",
            event.detail, event.event, self.drag_state.is_some());
        if let Some(drag) = self.drag_state.take() {
            tracing::debug!("Ending drag operation");
            self.conn.ungrab_pointer()?;

            match drag {
                DragState::TiledSwap { window, hover_target, workspace, original_tree, .. } => {
                    // Restore target border color
                    if let Some(target) = hover_target {
                        let color = if self.focused_window == Some(target) {
                            self.config.border_color_focused
                        } else {
                            self.config.border_color_unfocused
                        };
                        self.conn.set_border(target, self.config.border_width, color)?;
                    }

                    // Finalize: snap window to its layout position
                    if workspace == self.focused_workspace {
                        // Swaps already happened live during motion
                        // Just re-apply layout to snap dragged window to final position
                        self.apply_layout()?;
                        self.set_focus(window, true)?;
                    } else {
                        // Workspace changed during drag - revert to original
                        self.workspaces[workspace].tree = original_tree;
                        if workspace == self.focused_workspace {
                            self.apply_layout()?;
                        }
                    }
                }
                DragState::Move { window, .. } | DragState::Resize { window, .. } => {
                    // Restore edge cursor if pointer is still over the dragged floating window
                    if self.is_floating(window) {
                        self.update_edge_cursor(window, event.root_x, event.root_y)?;
                    }
                }
                DragState::TiledResize { .. } => {
                    // Layout already applied during motion, nothing special needed
                }
            }

            self.conn.flush()?;
        }
        Ok(())
    }

    fn handle_motion_notify(&mut self, event: MotionNotifyEvent) -> Result<()> {
        // If not in a drag, check for edge cursor changes on floating windows
        if self.drag_state.is_none() {
            let window = event.event;
            if self.windows.contains_key(&window) && self.is_floating(window) {
                self.update_edge_cursor(window, event.root_x, event.root_y)?;
            } else {
                // Check for tiled edge hover (for cursor feedback)
                self.update_tiled_edge_cursor(event.root_x, event.root_y)?;
            }
            return Ok(());
        }

        let drag = self.drag_state.as_ref().unwrap();
        tracing::debug!("Motion during drag: root({},{}), drag_state={:?}", event.root_x, event.root_y, drag);
        match drag {
            DragState::Move {
                window,
                start_x,
                start_y,
                start_geometry,
            } => {
                let dx = event.root_x - start_x;
                let dy = event.root_y - start_y;
                let new_x = start_geometry.x + dx;
                let new_y = start_geometry.y + dy;

                let window = *window;
                self.set_floating_position(window, new_x, new_y)?;
            }
            DragState::Resize {
                window,
                start_x,
                start_y,
                start_geometry,
                edge,
            } => {
                let dx = event.root_x - start_x;
                let dy = event.root_y - start_y;

                let (new_x, new_y, new_w, new_h) = calculate_resize(
                    start_geometry,
                    *edge,
                    dx,
                    dy,
                );

                let window = *window;
                self.set_floating_geometry(window, new_x, new_y, new_w, new_h)?;
            }
            DragState::TiledSwap {
                window,
                grab_offset_x,
                grab_offset_y,
                tiled_geometries,
                hover_target,
                workspace,
                ..
            } => {
                // Only process if still on same workspace
                if *workspace != self.focused_workspace {
                    return Ok(());
                }

                let cursor_x = event.root_x;
                let cursor_y = event.root_y;
                let dragged = *window;
                let old_target = *hover_target;
                let grab_offset_x = *grab_offset_x;
                let grab_offset_y = *grab_offset_y;

                // Move dragged window to follow cursor using fixed grab offset
                let new_x = cursor_x - grab_offset_x;
                let new_y = cursor_y - grab_offset_y;
                self.conn.move_window(dragged, new_x, new_y)?;
                self.conn.flush()?;

                // Find window under cursor (excluding dragged window)
                let new_target = tiled_geometries.iter()
                    .find(|(w, rect)| {
                        *w != dragged &&
                        cursor_x >= rect.x && cursor_x < rect.x + rect.width as i16 &&
                        cursor_y >= rect.y && cursor_y < rect.y + rect.height as i16
                    })
                    .map(|(w, _)| *w);

                // Perform live swap if target changed
                if new_target != old_target {
                    // Restore old target border
                    if let Some(old) = old_target {
                        let color = if self.focused_window == Some(old) {
                            self.config.border_color_focused
                        } else {
                            self.config.border_color_unfocused
                        };
                        self.conn.set_border(old, self.config.border_width, color)?;
                    }

                    // Perform actual swap if new target exists
                    if let Some(target) = new_target {
                        // Swap in tree and relayout (live preview)
                        self.current_workspace_mut().tree.swap(dragged, target);
                        self.apply_layout()?;

                        // Re-raise dragged window above others
                        self.conn.raise_window(dragged)?;

                        // Highlight new target
                        self.conn.set_border(target, self.config.border_width,
                            self.config.border_color_swap_target)?;

                        // Update cached geometries after swap
                        let screen = self.screen_rect();
                        let new_geometries = self.current_workspace()
                            .tree
                            .calculate_geometries(screen);
                        if let Some(DragState::TiledSwap { tiled_geometries: tg, hover_target: ht, .. }) = &mut self.drag_state {
                            *tg = new_geometries;
                            *ht = new_target;
                        }
                    } else {
                        // No new target, just update hover state
                        if let Some(DragState::TiledSwap { hover_target: ht, .. }) = &mut self.drag_state {
                            *ht = new_target;
                        }
                    }
                    self.conn.flush()?;
                }
            }
            DragState::TiledResize {
                direction,
                start_pos,
                start_ratio,
                window,
                container_size,
                workspace,
            } => {
                if *workspace != self.focused_workspace {
                    return Ok(());
                }

                let current_pos = match direction {
                    Direction::Left | Direction::Right => event.root_x,
                    Direction::Up | Direction::Down => event.root_y,
                };

                // Convert pixel delta to ratio delta
                let pixel_delta = current_pos - *start_pos;
                let ratio_delta = pixel_delta as f32 / *container_size as f32;

                let new_ratio = (*start_ratio + ratio_delta).clamp(0.1, 0.9);

                // Update tree and apply layout
                let window = *window;
                let direction = *direction;
                self.current_workspace_mut()
                    .tree
                    .set_split_ratio(window, direction, new_ratio);
                self.apply_layout()?;
                self.conn.flush()?;
            }
        }

        Ok(())
    }

    /// Handle pointer motion for edge cursor changes on floating windows.
    fn update_edge_cursor(&mut self, window: u32, root_x: i16, root_y: i16) -> Result<()> {
        if !self.is_floating(window) {
            // Not a floating window, clear any edge cursor state
            if let Some((old_window, old_edge)) = self.current_edge_cursor {
                if old_edge != ResizeEdge::None {
                    self.conn.clear_window_cursor(old_window)?;
                }
                self.current_edge_cursor = None;
            }
            return Ok(());
        }

        let geometry = self.get_floating_geometry(window);
        let edge = determine_resize_edge(&geometry, root_x, root_y);

        // Check if we need to update the cursor
        let current = self.current_edge_cursor;
        if current.map(|(w, e)| (w, e)) == Some((window, edge)) {
            return Ok(()); // No change needed
        }

        if edge != ResizeEdge::None {
            let cursor = match edge {
                ResizeEdge::TopLeft => self.conn.cursor_top_left,
                ResizeEdge::Top => self.conn.cursor_top,
                ResizeEdge::TopRight => self.conn.cursor_top_right,
                ResizeEdge::Left => self.conn.cursor_left,
                ResizeEdge::Right => self.conn.cursor_right,
                ResizeEdge::BottomLeft => self.conn.cursor_bottom_left,
                ResizeEdge::Bottom => self.conn.cursor_bottom,
                ResizeEdge::BottomRight => self.conn.cursor_bottom_right,
                ResizeEdge::None => unreachable!(),
            };
            self.conn.set_window_cursor(window, cursor)?;
            self.conn.flush()?;
            self.current_edge_cursor = Some((window, edge));
        } else if let Some((old_window, old_edge)) = current {
            if old_edge != ResizeEdge::None {
                self.conn.clear_window_cursor(old_window)?;
                self.conn.flush()?;
            }
            self.current_edge_cursor = Some((window, ResizeEdge::None));
        } else {
            self.current_edge_cursor = Some((window, ResizeEdge::None));
        }

        Ok(())
    }

    /// Update cursor when hovering over tiled window edges/gaps.
    fn update_tiled_edge_cursor(&mut self, root_x: i16, root_y: i16) -> Result<()> {
        let work_area = self.work_area();
        let geometries = self.current_workspace().tree.calculate_geometries(work_area);

        let edge = self.find_tiled_edge(root_x, root_y, &geometries);

        let new_direction = edge.map(|(_, dir, _)| dir);

        // Check if cursor state changed
        if new_direction == self.tiled_edge_cursor {
            return Ok(()); // No change
        }

        if let Some(dir) = new_direction {
            // Set resize cursor on root (double-arrow cursors)
            let cursor = match dir {
                Direction::Left | Direction::Right => self.conn.cursor_h_double,
                Direction::Up | Direction::Down => self.conn.cursor_v_double,
            };
            self.conn.set_window_cursor(self.conn.root, cursor)?;
            self.conn.flush()?;
        } else if self.tiled_edge_cursor.is_some() {
            // Clear resize cursor from root
            self.conn.clear_window_cursor(self.conn.root)?;
            self.conn.flush()?;
        }

        self.tiled_edge_cursor = new_direction;
        Ok(())
    }

    fn handle_enter_notify(&mut self, event: EnterNotifyEvent) -> Result<()> {
        let window = event.event;

        // Ignore if we're in a drag operation
        if self.drag_state.is_some() {
            return Ok(());
        }

        // Suppress EnterNotify events that happen shortly after a pointer warp
        // This prevents feedback loops from mouse-follows-focus
        if self.last_warp.elapsed() < std::time::Duration::from_millis(50) {
            return Ok(());
        }

        // Ignore inferior (entering from a child window) and non-normal modes
        // Only handle "Normal" mode enters (actual mouse movement)
        if event.mode != NotifyMode::NORMAL {
            return Ok(());
        }

        // Only focus windows we manage
        if !self.windows.contains_key(&window) {
            return Ok(());
        }

        // Don't focus if already focused
        if self.focused_window == Some(window) {
            return Ok(());
        }

        tracing::debug!("Focus follows mouse: focusing window {}", window);

        // Focus the new window (no warp - mouse enter)
        // set_focus handles grab/ungrab for old and new windows
        self.set_focus(window, false)?;

        // Raise floating windows on focus
        if self.is_floating(window) {
            self.raise_window(window)?;
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Handle EWMH client message requests (focus, workspace switch, close, state changes).
    fn handle_client_message(&mut self, event: ClientMessageEvent) -> Result<()> {
        let msg_type = event.type_;
        let window = event.window;

        if msg_type == self.conn.net_active_window {
            // Application requesting focus
            tracing::debug!("ClientMessage: _NET_ACTIVE_WINDOW for window {}", window);

            if self.windows.contains_key(&window) {
                // Get the window's workspace and switch to it if needed
                if let Some(win) = self.windows.get(&window) {
                    let ws_idx = win.workspace;
                    if ws_idx != self.focused_workspace {
                        self.switch_workspace(ws_idx)?;
                    }
                }
                // Focus the window (external activation, no warp - user is already interacting)
                self.set_focus(window, false)?;
                if self.is_floating(window) {
                    self.raise_window(window)?;
                }
            }
        } else if msg_type == self.conn.net_current_desktop {
            // Workspace switch request (from pagers, etc.)
            let desktop = event.data.as_data32()[0] as usize;
            tracing::debug!("ClientMessage: _NET_CURRENT_DESKTOP to {}", desktop);

            if desktop < self.workspaces.len() {
                self.switch_workspace(desktop)?;
            }
        } else if msg_type == self.conn.net_close_window {
            // Close window request
            tracing::debug!("ClientMessage: _NET_CLOSE_WINDOW for window {}", window);

            if self.windows.contains_key(&window) {
                self.close_window(window)?;
            }
        } else if msg_type == self.conn.net_wm_state {
            // Window state change request (fullscreen, etc.)
            let action = event.data.as_data32()[0];
            let property = event.data.as_data32()[1];
            tracing::debug!(
                "ClientMessage: _NET_WM_STATE action={} property={} for window {}",
                action, property, window
            );

            // Handle fullscreen state changes
            if property == self.conn.net_wm_state_fullscreen {
                // action: 0 = remove, 1 = add, 2 = toggle
                match action {
                    0 => {
                        // Remove fullscreen
                        self.set_fullscreen(window, false)?;
                    }
                    1 => {
                        // Add fullscreen
                        self.set_fullscreen(window, true)?;
                    }
                    2 => {
                        // Toggle fullscreen
                        self.toggle_fullscreen(window)?;
                    }
                    _ => {}
                }
            }
        } else {
            tracing::trace!("Unhandled ClientMessage type: {}", msg_type);
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Handle PropertyNotify events (urgency hints, etc.).
    fn handle_property_notify(&mut self, event: PropertyNotifyEvent) -> Result<()> {
        let window = event.window;
        let atom = event.atom;

        // Check if WM_HINTS changed (urgency flag may have changed)
        if atom == self.conn.wm_hints {
            // Only handle for managed windows
            if !self.windows.contains_key(&window) {
                return Ok(());
            }

            tracing::debug!("WM_HINTS changed for window {}", window);

            // Get the current WM_HINTS
            if let Some(hints) = self.conn.get_wm_hints(window) {
                let old_urgent = self.windows.get(&window).map(|w| w.urgent).unwrap_or(false);
                let new_urgent = hints.urgent;

                if old_urgent != new_urgent {
                    tracing::info!(
                        "Window {} urgency changed: {} -> {}",
                        window, old_urgent, new_urgent
                    );

                    // Update window state
                    if let Some(win) = self.windows.get_mut(&window) {
                        win.urgent = new_urgent;
                    }

                    // Update border colors
                    self.update_borders()?;
                    self.conn.flush()?;
                }
            }
        }

        Ok(())
    }

    /// Handle Expose events to redraw title bars.
    fn handle_expose(&mut self, event: ExposeEvent) -> Result<()> {
        let window = event.window;

        // Only process when count is 0 (last expose in batch)
        if event.count != 0 {
            return Ok(());
        }

        // Check if this is a frame window
        if let Some(client) = self.frames.client_for_frame(window) {
            // Redraw the title bar
            if self.config.titlebar_enabled {
                let win_state = self.windows.get(&client);
                let title = win_state.map(|w| w.title.as_str()).unwrap_or("");
                let focused = self.focused_window == Some(client);

                let bg_color = if focused {
                    self.config.titlebar_color_focused
                } else {
                    self.config.titlebar_color_unfocused
                };

                // Get frame width from expose event
                let width = event.width;
                let titlebar_height = self.config.titlebar_height as u16;

                self.frames.draw_titlebar(
                    &self.conn.conn,
                    client,
                    title,
                    width,
                    titlebar_height,
                    bg_color,
                    self.config.titlebar_text_color,
                    focused,
                )?;

                self.conn.flush()?;
            }
        }

        Ok(())
    }

    fn handle_key_press(&mut self, event: KeyPressEvent) -> Result<()> {
        let keycode = event.detail;
        let state = event.state;

        // Convert KeyButMask to ModMask for comparison
        // Filter out NumLock (M2), CapsLock (Lock), and ScrollLock (M5)
        let modifiers = ModMask::from(
            (state.bits() & (ModMask::SHIFT | ModMask::CONTROL | ModMask::M1 | ModMask::M4).bits())
                as u16,
        );

        tracing::trace!("KeyPress: keycode={}, raw_state={:?}, filtered_mods={:?}",
            keycode, state, modifiers);

        // Find matching keybind from Lua config
        let action = {
            let lua_state = self.lua_state.lock().unwrap();
            lua_state
                .keybinds
                .iter()
                .find(|kb| {
                    let bind_keycode = self.conn.keycode_from_keysym(kb.keysym);
                    bind_keycode == Some(keycode) && kb.modifiers == modifiers
                })
                .map(|kb| kb.action.clone())
        };

        if let Some(action) = action {
            tracing::debug!("Executing action: {:?}", action);
            self.execute_action(action)?;
        }

        Ok(())
    }

    fn execute_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Exec(cmd) => {
                tracing::info!("Exec: {}", cmd);
                Command::new("sh").arg("-c").arg(&cmd).spawn().ok();
            }
            Action::CloseWindow => {
                if let Some(window) = self.focused_window {
                    self.close_window(window)?;
                }
            }
            Action::ForceCloseWindow => {
                if let Some(window) = self.focused_window {
                    self.force_close_window(window)?;
                }
            }
            Action::Focus(direction) => {
                if let Some(dir) = parse_direction(&direction) {
                    self.focus_direction(dir)?;
                }
            }
            Action::Swap(direction) => {
                if let Some(dir) = parse_direction(&direction) {
                    self.swap_direction(dir)?;
                }
            }
            Action::Resize(direction, amount) => {
                if let Some(dir) = parse_direction(&direction) {
                    self.resize_direction(dir, amount)?;
                }
            }
            Action::Equalize => {
                self.equalize()?;
            }
            Action::Workspace(idx) => {
                // Lua uses 1-based indexing
                self.switch_workspace(idx.saturating_sub(1))?;
            }
            Action::WorkspaceNext => {
                // Find next workspace with windows, wrapping around
                let len = self.workspaces.len();
                for i in 1..=len {
                    let idx = (self.focused_workspace + i) % len;
                    if self.workspaces[idx].has_windows() {
                        self.switch_workspace(idx)?;
                        break;
                    }
                }
            }
            Action::WorkspacePrev => {
                // Find previous workspace with windows, wrapping around
                let len = self.workspaces.len();
                for i in 1..=len {
                    let idx = (self.focused_workspace + len - i) % len;
                    if self.workspaces[idx].has_windows() {
                        self.switch_workspace(idx)?;
                        break;
                    }
                }
            }
            Action::MoveToWorkspace(idx) => {
                // Lua uses 1-based indexing
                self.move_to_workspace(idx.saturating_sub(1))?;
            }
            Action::Reload => {
                self.reload_config()?;
            }
            Action::Exit => {
                tracing::info!("Exit requested, will exit event loop");
                self.running = false;
                // Force an immediate return from event handling
                return Ok(());
            }
            Action::ToggleFloating => {
                if let Some(window) = self.focused_window {
                    self.toggle_floating(window)?;
                }
            }
            Action::ToggleFullscreen => {
                if let Some(window) = self.focused_window {
                    self.toggle_fullscreen(window)?;
                }
            }
            Action::CycleFloating => {
                self.cycle_floating()?;
            }
            Action::FocusMonitor(target) => {
                self.focus_monitor(&target)?;
            }
            Action::MoveToMonitor(target) => {
                self.move_to_monitor(&target)?;
            }
            Action::LuaCallback(index) => {
                if let Err(e) = self.lua_config.execute_callback(index) {
                    tracing::error!("Lua callback error: {}", e);
                }
            }
        }
        Ok(())
    }

    fn close_window(&mut self, window: u32) -> Result<()> {
        // Verify we actually have a window to close
        if !self.windows.contains_key(&window) {
            tracing::warn!("close_window called on unmanaged window {}", window);
            return Ok(());
        }

        tracing::info!("Closing window {}", window);

        // Try graceful ICCCM close first
        if self.conn.supports_delete_window(window) {
            tracing::info!("Window {} supports WM_DELETE_WINDOW, sending graceful close", window);
            self.conn.send_delete_window(window)?;
        } else {
            tracing::info!("Window {} doesn't support WM_DELETE_WINDOW, using kill_client", window);
            self.conn.conn.kill_client(window)?;
        }

        self.conn.flush()?;
        Ok(())
    }

    fn force_close_window(&mut self, window: u32) -> Result<()> {
        tracing::info!("Force closing window {}", window);
        self.conn.conn.kill_client(window)?;
        self.conn.flush()?;
        Ok(())
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        let Some(focused) = self.focused_window else {
            // No focused window - try to focus adjacent monitor
            return self.focus_adjacent_monitor(direction);
        };

        let screen = self.screen_rect();
        let geometries = self.current_workspace().tree.calculate_geometries(screen);

        // Look up remembered window for this direction (window memory)
        let preferred = self.directional_focus_memory.get(&(focused, direction)).copied();

        if let Some(target) = Node::find_adjacent(&geometries, focused, direction, preferred) {
            // Check if memory was used (preferred matched target) or default algorithm was used
            let used_memory = preferred == Some(target);

            // Store the directional focus memory for next time
            self.directional_focus_memory.insert((focused, direction), target);

            // Store reverse direction only if:
            // 1. Default algorithm was used (not memory-assisted jump that skipped windows)
            // 2. Windows are aligned (same row/column)
            if !used_memory {
                if let (Some((_, from_rect)), Some((_, to_rect))) = (
                    geometries.iter().find(|(w, _)| *w == focused),
                    geometries.iter().find(|(w, _)| *w == target),
                ) {
                    let dominated = match direction {
                        // For Left/Right: store reverse if windows share vertical space (same row)
                        Direction::Left | Direction::Right => {
                            let overlap_start = from_rect.y.max(to_rect.y);
                            let overlap_end = (from_rect.y + from_rect.height as i16)
                                .min(to_rect.y + to_rect.height as i16);
                            overlap_start < overlap_end
                        }
                        // For Up/Down: store reverse if windows share horizontal space (same column)
                        Direction::Up | Direction::Down => {
                            let overlap_start = from_rect.x.max(to_rect.x);
                            let overlap_end = (from_rect.x + from_rect.width as i16)
                                .min(to_rect.x + to_rect.width as i16);
                            overlap_start < overlap_end
                        }
                    };

                    if dominated {
                        let opposite = match direction {
                            Direction::Left => Direction::Right,
                            Direction::Right => Direction::Left,
                            Direction::Up => Direction::Down,
                            Direction::Down => Direction::Up,
                        };
                        self.directional_focus_memory.insert((target, opposite), focused);
                    }
                }
            }

            // Focus new window (keyboard navigation, warp pointer)
            // set_focus handles grab/ungrab for old and new windows
            self.set_focus(target, true)?;
            self.conn.flush()?;

            tracing::debug!("Focused {:?} to window {} (preferred: {:?})", direction, target, preferred);
        } else {
            // No adjacent window on this workspace - try adjacent monitor
            self.focus_adjacent_monitor(direction)?;
        }

        Ok(())
    }

    /// Focus the adjacent monitor in the given direction (does NOT wrap at edges)
    fn focus_adjacent_monitor(&mut self, direction: Direction) -> Result<()> {
        if self.monitors.len() <= 1 {
            return Ok(());
        }

        // Calculate target index WITHOUT wrapping
        let target_idx = match direction {
            Direction::Left => {
                if self.focused_monitor == 0 {
                    // At leftmost monitor - do nothing
                    return Ok(());
                }
                self.focused_monitor - 1
            }
            Direction::Right => {
                if self.focused_monitor >= self.monitors.len() - 1 {
                    // At rightmost monitor - do nothing
                    return Ok(());
                }
                self.focused_monitor + 1
            }
            // Up/Down could navigate if monitors are stacked vertically
            Direction::Up | Direction::Down => return Ok(()),
        };

        tracing::info!("Moving focus from monitor {} to {}", self.focused_monitor, target_idx);
        self.focused_monitor = target_idx;

        // Focus the active workspace on that monitor
        let workspace_idx = self.monitors[target_idx].active_workspace;
        self.focused_workspace = workspace_idx;

        // Focus a window on that workspace if any, or just warp to monitor center
        if let Some(window) = self.workspaces[workspace_idx].focused
            .or_else(|| self.workspaces[workspace_idx].floating.last().copied())
            .or_else(|| self.workspaces[workspace_idx].tree.first_window())
        {
            // set_focus handles grab/ungrab for old and new windows
            self.set_focus(window, true)?;
        } else {
            // No windows on target monitor - clear focus and warp to monitor center
            self.focused_window = None;
            self.warp_to_monitor(target_idx)?;
            tracing::debug!("No windows on monitor {}, warped to center", target_idx);
        }

        self.conn.flush()?;
        Ok(())
    }

    fn swap_direction(&mut self, direction: Direction) -> Result<()> {
        let Some(focused) = self.focused_window else {
            return Ok(());
        };

        let screen = self.screen_rect();
        let geometries = self.current_workspace().tree.calculate_geometries(screen);

        if let Some(target) = Node::find_adjacent(&geometries, focused, direction, None) {
            // Swap the windows in the tree
            self.current_workspace_mut().tree.swap(focused, target);

            // Re-apply layout
            self.apply_layout()?;

            // Keep focus on the original window (now in swapped position)
            self.set_focus(focused, true)?;

            tracing::debug!("Swapped with window {} in direction {:?}", target, direction);
        } else if self.monitors.len() > 1 {
            // No adjacent window - try moving to adjacent monitor
            let target_monitor = match direction {
                Direction::Left => Some("prev"),
                Direction::Right => Some("next"),
                // For up/down with horizontal monitor arrangement, could also try prev/next
                // but typically vertical movement doesn't cross monitors
                Direction::Up | Direction::Down => None,
            };

            if let Some(target) = target_monitor {
                tracing::debug!("No adjacent window, moving to {} monitor", target);
                self.move_to_monitor(target)?;
            }
        }

        Ok(())
    }

    fn resize_direction(&mut self, direction: Direction, delta: f32) -> Result<()> {
        let Some(focused) = self.focused_window else {
            return Ok(());
        };

        // Resize the split
        if self
            .current_workspace_mut()
            .tree
            .resize(focused, direction, delta)
        {
            // Re-apply layout
            self.apply_layout()?;
            tracing::debug!("Resized {:?}", direction);
        }

        Ok(())
    }

    fn equalize(&mut self) -> Result<()> {
        self.current_workspace_mut().tree.equalize();
        self.apply_layout()?;
        tracing::debug!("Equalized splits");
        Ok(())
    }

    /// Execute an i3-compatible command (from IPC RUN_COMMAND).
    /// Returns true if the command was executed successfully.
    fn execute_i3_command(&mut self, cmd: &str) -> bool {
        let cmd = cmd.trim();
        tracing::debug!("Executing i3 command: {}", cmd);

        // Parse "workspace <name|number>" command
        // Use switch_workspace_impl with warp_pointer=false since IPC commands
        // (like clicks from the bar) shouldn't move the mouse cursor
        if let Some(rest) = cmd.strip_prefix("workspace ") {
            let rest = rest.trim();
            // Try to parse as number first
            if let Ok(num) = rest.parse::<usize>() {
                // Workspace numbers are 1-indexed in i3
                let idx = num.saturating_sub(1);
                if idx < self.workspaces.len() {
                    if let Err(e) = self.switch_workspace_impl(idx, false) {
                        tracing::warn!("Failed to switch workspace: {}", e);
                        return false;
                    }
                    return true;
                }
            }
            // Try to match by name
            if let Some(idx) = self.workspaces.iter().position(|ws| ws.name == rest) {
                if let Err(e) = self.switch_workspace_impl(idx, false) {
                    tracing::warn!("Failed to switch workspace: {}", e);
                    return false;
                }
                return true;
            }
            tracing::warn!("Workspace not found: {}", rest);
            return false;
        }

        // Parse "workspace number <n>" command
        if let Some(rest) = cmd.strip_prefix("workspace number ") {
            if let Ok(num) = rest.trim().parse::<usize>() {
                let idx = num.saturating_sub(1);
                if idx < self.workspaces.len() {
                    if let Err(e) = self.switch_workspace_impl(idx, false) {
                        tracing::warn!("Failed to switch workspace: {}", e);
                        return false;
                    }
                    return true;
                }
            }
            return false;
        }

        tracing::debug!("Unknown i3 command: {}", cmd);
        false
    }

    /// Switch to workspace using i3-style behavior:
    /// - If workspace is visible on another monitor, focus moves to that monitor
    /// - If workspace is not visible, it appears on the current monitor
    /// If warp_pointer is false, the mouse cursor is not moved (for IPC commands).
    fn switch_workspace_impl(&mut self, idx: usize, warp_pointer: bool) -> Result<()> {
        if idx >= self.workspaces.len() {
            return Ok(());
        }

        // Track old workspace for event broadcasting
        let old_workspace_idx = self.focused_workspace;

        // Check if workspace is already visible on some monitor
        let visible_on_monitor = self.monitors.iter().position(|m| m.active_workspace == idx);

        if let Some(monitor_idx) = visible_on_monitor {
            // Workspace is already visible - just focus that monitor (i3 behavior)
            if monitor_idx == self.focused_monitor {
                // Already on this workspace on this monitor
                return Ok(());
            }

            tracing::info!(
                "Workspace {} already visible on monitor {}, focusing it",
                idx + 1, monitor_idx
            );

            // Focus the monitor that has this workspace
            self.focused_monitor = monitor_idx;
            self.focused_workspace = idx;

            // Update EWMH
            self.conn.set_current_desktop(idx as u32)?;

            // Warp pointer to that monitor (only if requested)
            if warp_pointer {
                let monitor_geom = self.monitors[monitor_idx].geometry;
                let center_x = monitor_geom.x + (monitor_geom.width as i16 / 2);
                let center_y = monitor_geom.y + (monitor_geom.height as i16 / 2);
                self.conn.warp_pointer(center_x, center_y)?;
                self.last_warp = std::time::Instant::now();
            }

            // Focus a window on that workspace
            if let Some(window) = self.workspaces[idx].focused
                .or_else(|| self.workspaces[idx].floating.last().copied())
                .or_else(|| self.workspaces[idx].tree.first_window())
            {
                self.set_focus(window, warp_pointer)?;
            } else {
                self.focused_window = None;
                self.conn.set_active_window(None)?;
            }
        } else {
            // Workspace not visible - show it on current monitor (i3 behavior)
            let current_monitor = self.focused_monitor;
            let old_ws = self.monitors[current_monitor].active_workspace;

            tracing::info!(
                "Switching monitor {} from workspace {} to {}",
                current_monitor, old_ws + 1, idx + 1
            );

            // Hide windows on old workspace
            for window in self.workspaces[old_ws].all_windows() {
                // Mark as intentional unmap so UnmapNotify handler ignores it
                if let Some(win) = self.windows.get_mut(&window) {
                    win.ignore_unmap_count += 1;
                }
                self.conn.unmap_window(window)?;
                // Also unmap frames if present
                if let Some(frame) = self.frames.frame_for_client(window) {
                    self.conn.unmap_window(frame)?;
                }
            }

            // Update monitor's active workspace
            self.monitors[current_monitor].active_workspace = idx;
            self.focused_workspace = idx;

            // Update EWMH
            self.conn.set_current_desktop(idx as u32)?;

            // Show windows on new workspace
            for window in self.workspaces[idx].all_windows() {
                if let Some(frame) = self.frames.frame_for_client(window) {
                    self.conn.map_window(frame)?;
                }
                self.conn.map_window(window)?;
            }

            // Apply layout
            self.apply_layout()?;

            // Focus a window on the new workspace
            if let Some(window) = self.workspaces[idx].focused
                .or_else(|| self.workspaces[idx].floating.last().copied())
                .or_else(|| self.workspaces[idx].tree.first_window())
            {
                self.set_focus(window, warp_pointer)?;
            } else {
                // No windows - warp to center of monitor (only if requested)
                self.focused_window = None;
                self.conn.set_active_window(None)?;
                if warp_pointer {
                    let monitor_geom = self.monitors[current_monitor].geometry;
                    let center_x = monitor_geom.x + (monitor_geom.width as i16 / 2);
                    let center_y = monitor_geom.y + (monitor_geom.height as i16 / 2);
                    self.conn.warp_pointer(center_x, center_y)?;
                    self.last_warp = std::time::Instant::now();
                }
            }
        }

        // Broadcast i3 workspace event for polybar
        if idx != old_workspace_idx {
            self.broadcast_i3_workspace_event("focus", idx, Some(old_workspace_idx));
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Switch to workspace with pointer warping (default behavior for keybinds).
    fn switch_workspace(&mut self, idx: usize) -> Result<()> {
        self.switch_workspace_impl(idx, true)
    }

    fn move_to_workspace(&mut self, idx: usize) -> Result<()> {
        if idx >= self.workspaces.len() {
            return Ok(());
        }

        let Some(window) = self.focused_window else {
            return Ok(());
        };

        // Get current workspace for this window
        let current_ws = self.windows.get(&window).map(|w| w.workspace).unwrap_or(self.focused_workspace);

        // Don't move if already on target workspace
        if idx == current_ws {
            return Ok(());
        }

        let is_floating = self.windows.get(&window).map(|w| w.floating).unwrap_or(false);

        tracing::info!("Moving window {} from workspace {} to {} (floating: {})",
            window, current_ws + 1, idx + 1, is_floating);

        // Remove from current workspace (tree or floating list)
        if is_floating {
            self.workspaces[current_ws].remove_floating(window);
        } else {
            self.workspaces[current_ws].tree.remove(window);
        }

        // Update focus on current workspace
        let new_focus_on_current = self.workspaces[current_ws].tree.first_window()
            .or_else(|| self.workspaces[current_ws].floating.last().copied());
        self.workspaces[current_ws].focused = new_focus_on_current;

        // Update window's workspace tracking
        if let Some(win) = self.windows.get_mut(&window) {
            win.workspace = idx;
        }

        // Update EWMH _NET_WM_DESKTOP
        self.conn.set_window_desktop(window, idx as u32)?;

        // Check if target workspace is visible on any monitor
        let target_visible_on = self.monitors.iter().position(|m| m.active_workspace == idx);

        // Insert into target workspace
        if is_floating {
            self.workspaces[idx].add_floating(window);
        } else {
            let target_focused = self.workspaces[idx].focused;
            // Use target monitor's geometry if visible, otherwise use current monitor's
            let screen = if let Some(mon_idx) = target_visible_on {
                self.monitors[mon_idx].geometry
            } else {
                self.monitors[self.focused_monitor].geometry
            };
            self.workspaces[idx]
                .tree
                .insert_with_rect(window, target_focused, screen);
        }

        // Set target workspace focus to the moved window so switch_workspace will focus it
        self.workspaces[idx].focused = Some(window);

        // If target workspace is visible, map the window; otherwise hide it
        if target_visible_on.is_some() {
            // Target is visible - map the window
            if let Some(frame) = self.frames.frame_for_client(window) {
                self.conn.map_window(frame)?;
            }
            self.conn.map_window(window)?;
        } else {
            // Target is not visible - hide the window
            // Mark as intentional unmap so UnmapNotify handler ignores it
            if let Some(win) = self.windows.get_mut(&window) {
                win.ignore_unmap_count += 1;
            }
            self.conn.unmap_window(window)?;
            if let Some(frame) = self.frames.frame_for_client(window) {
                self.conn.unmap_window(frame)?;
            }
        }

        // Re-apply layout
        self.apply_layout()?;

        // Check if we should follow the window to the target workspace
        if self.config.follow_window_on_move {
            // Switch to target workspace (this will focus the moved window)
            self.switch_workspace(idx)?;
        } else {
            // Stay on current workspace, update focus to next window
            if current_ws == self.focused_workspace {
                self.focused_window = new_focus_on_current;
                if let Some(new_focus) = self.focused_window {
                    self.set_focus(new_focus, true)?;
                } else {
                    self.conn.set_active_window(None)?;
                }
            }
        }

        self.conn.flush()?;
        Ok(())
    }

    fn reload_config(&mut self) -> Result<()> {
        tracing::info!("Reloading configuration");

        // Ungrab all current keys
        // (We'd need to track grabbed keys to ungrab them properly,
        // for now we'll just regrab - X11 handles duplicates)

        // Reload Lua config
        if let Err(e) = self.lua_config.reload() {
            tracing::error!("Config reload failed: {}", e);
            return Ok(());
        }

        // Update config from Lua state
        {
            let state = self.lua_state.lock().unwrap();
            self.config = state.config.clone();
        }

        // Regenerate picom config and signal picom to reload
        if let Err(e) = self.config.write_picom_config() {
            tracing::warn!("Failed to regenerate picom config: {}", e);
        }

        // Re-register keybinds
        self.setup_grabs()?;

        // Re-apply layout with new settings
        self.apply_layout()?;

        // Handle garbar lifecycle based on new config
        if self.config.bar_enabled {
            // Check if garbar is still running
            let garbar_alive = if let Some(ref mut child) = self.garbar_process {
                match child.try_wait() {
                    Ok(None) => true,  // Still running
                    Ok(Some(status)) => {
                        tracing::info!("garbar exited with status {}, will respawn", status);
                        false
                    }
                    Err(e) => {
                        tracing::warn!("Failed to check garbar status: {}", e);
                        false
                    }
                }
            } else {
                false
            };

            if garbar_alive {
                // garbar running, signal it to reload
                if let Some(ref child) = self.garbar_process {
                    reload_garbar(child);
                }
            } else {
                // garbar not running, spawn it
                self.garbar_process = spawn_garbar();
            }
        } else if let Some(ref mut child) = self.garbar_process {
            // bar_enabled is now false, stop garbar
            stop_garbar(child);
            self.garbar_process = None;
        }

        tracing::info!("Configuration reloaded");
        Ok(())
    }

    pub fn run(&mut self) -> Result<()> {
        tracing::info!("Starting event loop");

        // Set up keybinds
        self.setup_grabs()?;

        // Set up EWMH workspace hints
        self.setup_ewmh_hints()?;

        // Adopt any existing windows
        self.adopt_existing_windows()?;

        // Signal systemd that graphical session has started
        // This allows user services (like garbg) bound to graphical-session.target to start
        start_graphical_session();

        // Spawn garbar if gar.bar is configured
        if self.config.bar_enabled {
            self.garbar_process = spawn_garbar();
        }

        while self.running {
            // Handle X11 events (non-blocking poll)
            while let Some(event) = self.conn.conn.poll_for_event()? {
                self.handle_event(event)?;
            }

            // Handle IPC requests
            self.handle_ipc()?;

            // Handle i3-compatible IPC requests (for polybar)
            self.handle_i3_ipc()?;

            // Reap any zombie child processes (from exec/exec_once)
            reap_zombies();

            // Small sleep to avoid busy-waiting when idle
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Unmap all managed windows so they don't persist on the X server
        // This ensures windows aren't visible when returning to the greeter
        tracing::info!("Unmapping {} managed windows", self.windows.len());
        for &window in self.windows.keys() {
            tracing::debug!("Unmapping window {}", window);
            let _ = self.conn.conn.unmap_window(window);
        }
        // Sync to ensure X server processes all unmap requests before we exit
        let _ = self.conn.sync();
        tracing::info!("Windows unmapped and synced");

        // Stop garbar if it was spawned
        if let Some(ref mut child) = self.garbar_process {
            stop_garbar(child);
        }
        self.garbar_process = None;

        // Kill picom to prevent compositor effects from bleeding into the greeter
        tracing::info!("Killing picom...");
        let _ = std::process::Command::new("pkill")
            .arg("-x")
            .arg("picom")
            .status();

        // Signal systemd that graphical session has ended
        // This stops user services bound to graphical-session.target (like garbg)
        stop_graphical_session();

        tracing::info!("Event loop exited");
        Ok(())
    }

    /// Handle pending IPC requests
    fn handle_ipc(&mut self) -> Result<()> {
        let Some(ref mut ipc) = self.ipc_server else {
            return Ok(());
        };

        // Accept new connections
        ipc.accept_connections();

        // Process requests
        let requests = ipc.poll_requests();
        for (client_idx, request) in requests {
            let response = self.dispatch_ipc_command(&request.command, request.args);
            if let Some(ref mut ipc) = self.ipc_server {
                ipc.send_response(client_idx, response);
            }
        }

        Ok(())
    }

    /// Dispatch an IPC command and return a response
    fn dispatch_ipc_command(&mut self, command: &str, args: serde_json::Value) -> crate::ipc::Response {
        use crate::ipc::Response;

        match command {
            "focus" => {
                let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(dir) = parse_direction(direction) {
                    match self.focus_direction(dir) {
                        Ok(_) => Response::success(None),
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    Response::error(format!("Invalid direction: {}", direction))
                }
            }
            "swap" => {
                let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(dir) = parse_direction(direction) {
                    match self.swap_direction(dir) {
                        Ok(_) => Response::success(None),
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    Response::error(format!("Invalid direction: {}", direction))
                }
            }
            "resize" => {
                let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("");
                let amount = args.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.05) as f32;
                if let Some(dir) = parse_direction(direction) {
                    match self.resize_direction(dir, amount) {
                        Ok(_) => Response::success(None),
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    Response::error(format!("Invalid direction: {}", direction))
                }
            }
            "close" => {
                if let Some(window) = self.focused_window {
                    match self.close_window(window) {
                        Ok(_) => Response::success(None),
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    Response::error("No focused window")
                }
            }
            "workspace" => {
                let n = args.get("number").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                match self.switch_workspace(n.saturating_sub(1)) {
                    Ok(_) => Response::success(None),
                    Err(e) => Response::error(e.to_string()),
                }
            }
            "move_to_workspace" => {
                let n = args.get("number").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                match self.move_to_workspace(n.saturating_sub(1)) {
                    Ok(_) => Response::success(None),
                    Err(e) => Response::error(e.to_string()),
                }
            }
            "toggle_floating" => {
                if let Some(window) = self.focused_window {
                    match self.toggle_floating(window) {
                        Ok(_) => Response::success(None),
                        Err(e) => Response::error(e.to_string()),
                    }
                } else {
                    Response::error("No focused window")
                }
            }
            "equalize" => {
                match self.equalize() {
                    Ok(_) => Response::success(None),
                    Err(e) => Response::error(e.to_string()),
                }
            }
            "reload" => {
                match self.reload_config() {
                    Ok(_) => Response::success(None),
                    Err(e) => Response::error(e.to_string()),
                }
            }
            "exit" => {
                self.running = false;
                Response::success(None)
            }
            "get_workspaces" => {
                Response::success(Some(self.get_workspaces_json()))
            }
            "get_focused" => {
                Response::success(Some(self.get_focused_json()))
            }
            "get_tree" => {
                Response::success(Some(self.get_tree_json()))
            }
            "subscribe" => {
                // Handle subscription in handle_ipc directly
                Response::success(None)
            }
            "focus_monitor" => {
                let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("next");
                match self.focus_monitor(target) {
                    Ok(_) => Response::success(None),
                    Err(e) => Response::error(e.to_string()),
                }
            }
            "move_to_monitor" => {
                let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("next");
                match self.move_to_monitor(target) {
                    Ok(_) => Response::success(None),
                    Err(e) => Response::error(e.to_string()),
                }
            }
            "get_monitors" => {
                Response::success(Some(self.get_monitors_json()))
            }
            _ => Response::error(format!("Unknown command: {}", command)),
        }
    }

    /// Get workspace info as JSON
    fn get_workspaces_json(&self) -> serde_json::Value {
        serde_json::json!(self.workspaces.iter().enumerate().map(|(i, ws)| {
            serde_json::json!({
                "id": ws.id,
                "name": ws.name,
                "focused": i == self.focused_workspace,
                "tiled_count": ws.tree.window_count(),
                "floating_count": ws.floating.len(),
            })
        }).collect::<Vec<_>>())
    }

    /// Get focused window info as JSON
    fn get_focused_json(&self) -> serde_json::Value {
        match self.focused_window {
            Some(win_id) => {
                if let Some(win) = self.windows.get(&win_id) {
                    serde_json::json!({
                        "id": win_id,
                        "workspace": win.workspace + 1,
                        "floating": win.floating,
                    })
                } else {
                    serde_json::json!(null)
                }
            }
            None => serde_json::json!(null),
        }
    }

    /// Get window tree as JSON
    fn get_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "focused_workspace": self.focused_workspace + 1,
            "workspaces": self.workspaces.iter().map(|ws| {
                serde_json::json!({
                    "id": ws.id,
                    "name": ws.name,
                    "tiled": ws.tree.windows(),
                    "floating": ws.floating,
                })
            }).collect::<Vec<_>>()
        })
    }

    /// Get monitor info as JSON
    fn get_monitors_json(&self) -> serde_json::Value {
        serde_json::json!(self.monitors.iter().enumerate().map(|(i, mon)| {
            serde_json::json!({
                "name": mon.name,
                "focused": i == self.focused_monitor,
                "primary": mon.primary,
                "geometry": {
                    "x": mon.geometry.x,
                    "y": mon.geometry.y,
                    "width": mon.geometry.width,
                    "height": mon.geometry.height,
                },
                "workspaces": mon.workspaces.iter().map(|ws| ws + 1).collect::<Vec<_>>(),
                "active_workspace": mon.active_workspace + 1,
            })
        }).collect::<Vec<_>>())
    }

    // Floating window helpers

    fn is_floating(&self, window: u32) -> bool {
        self.windows
            .get(&window)
            .map(|w| w.floating)
            .unwrap_or(false)
    }

    /// Get the appropriate cursor for a resize edge.
    fn cursor_for_edge(&self, edge: ResizeEdge) -> u32 {
        match edge {
            ResizeEdge::TopLeft => self.conn.cursor_top_left,
            ResizeEdge::Top => self.conn.cursor_top,
            ResizeEdge::TopRight => self.conn.cursor_top_right,
            ResizeEdge::Left => self.conn.cursor_left,
            ResizeEdge::Right => self.conn.cursor_right,
            ResizeEdge::BottomLeft => self.conn.cursor_bottom_left,
            ResizeEdge::Bottom => self.conn.cursor_bottom,
            ResizeEdge::BottomRight => self.conn.cursor_bottom_right,
            ResizeEdge::None => self.conn.cursor_normal,
        }
    }

    /// Find if cursor is in the gap between two adjacent tiled windows.
    /// Returns (window, direction, container_size) if on a valid shared edge.
    /// Only matches gaps between windows, NOT outer edges.
    fn find_tiled_edge(
        &self,
        x: i16,
        y: i16,
        geometries: &[(u32, Rect)],
    ) -> Option<(u32, Direction, u16)> {
        const TILED_EDGE_THRESHOLD: i16 = 16;
        let gap_tolerance = self.config.gap_inner as i16 + 4;

        for (w1, r1) in geometries {
            for (w2, r2) in geometries {
                if w1 >= w2 {
                    continue;
                } // Avoid duplicate pairs

                // Check for vertical shared edge (windows side by side)
                // r1's right edge meets r2's left edge
                let r1_right = r1.x + r1.width as i16;

                if (r1_right - r2.x).abs() <= gap_tolerance && r1_right <= r2.x {
                    // Verify vertical overlap (they share a horizontal span)
                    let y_min = r1.y.max(r2.y);
                    let y_max = (r1.y + r1.height as i16).min(r2.y + r2.height as i16);
                    if y_min < y_max && y >= y_min && y < y_max {
                        // Cursor must be in the gap between them
                        let gap_center = (r1_right + r2.x) / 2;
                        if (x - gap_center).abs() <= TILED_EDGE_THRESHOLD {
                            let container_width = (r1.width + r2.width) as u16;
                            return Some((*w1, Direction::Right, container_width));
                        }
                    }
                }

                // Check for horizontal shared edge (windows stacked vertically)
                // r1's bottom edge meets r2's top edge
                let r1_bottom = r1.y + r1.height as i16;

                if (r1_bottom - r2.y).abs() <= gap_tolerance && r1_bottom <= r2.y {
                    // Verify horizontal overlap (they share a vertical span)
                    let x_min = r1.x.max(r2.x);
                    let x_max = (r1.x + r1.width as i16).min(r2.x + r2.width as i16);
                    if x_min < x_max && x >= x_min && x < x_max {
                        // Cursor must be in the gap between them
                        let gap_center = (r1_bottom + r2.y) / 2;
                        if (y - gap_center).abs() <= TILED_EDGE_THRESHOLD {
                            let container_height = (r1.height + r2.height) as u16;
                            return Some((*w1, Direction::Down, container_height));
                        }
                    }
                }
            }
        }
        None
    }

    fn get_floating_geometry(&self, window: u32) -> Rect {
        self.windows
            .get(&window)
            .map(|w| w.floating_geometry)
            .unwrap_or_default()
    }

    fn set_floating_position(&mut self, window: u32, x: i16, y: i16) -> Result<()> {
        if let Some(win) = self.windows.get_mut(&window) {
            win.floating_geometry.x = x;
            win.floating_geometry.y = y;
            self.conn.configure_window(
                window,
                x,
                y,
                win.floating_geometry.width,
                win.floating_geometry.height,
                self.config.border_width,
            )?;
            self.conn.flush()?;
        }
        Ok(())
    }

    fn set_floating_geometry(&mut self, window: u32, x: i16, y: i16, w: u16, h: u16) -> Result<()> {
        if let Some(win) = self.windows.get_mut(&window) {
            win.floating_geometry = Rect::new(x, y, w, h);
            self.conn.configure_window(
                window,
                x,
                y,
                w,
                h,
                self.config.border_width,
            )?;
            self.conn.flush()?;
        }
        Ok(())
    }

    fn raise_window(&mut self, window: u32) -> Result<()> {
        // Update stacking order in workspace's floating list
        self.current_workspace_mut().raise_floating(window);

        // Raise in X11
        let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        self.conn.conn.configure_window(window, &aux)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Focus a different monitor.
    /// Target can be "next", "prev", "left", "right", or a monitor name.
    fn focus_monitor(&mut self, target: &str) -> Result<()> {
        if self.monitors.len() <= 1 {
            return Ok(());
        }

        let target_idx = match target.to_lowercase().as_str() {
            "next" | "right" => (self.focused_monitor + 1) % self.monitors.len(),
            "prev" | "left" => {
                if self.focused_monitor == 0 {
                    self.monitors.len() - 1
                } else {
                    self.focused_monitor - 1
                }
            }
            name => {
                // Find monitor by name
                match self.monitors.iter().position(|m| m.name.eq_ignore_ascii_case(name)) {
                    Some(idx) => idx,
                    None => {
                        tracing::warn!("Monitor '{}' not found", name);
                        return Ok(());
                    }
                }
            }
        };

        if target_idx == self.focused_monitor {
            return Ok(());
        }

        tracing::info!("Focusing monitor {}: '{}'", target_idx, self.monitors[target_idx].name);
        self.focused_monitor = target_idx;

        // Focus the active workspace on that monitor
        let workspace_idx = self.monitors[target_idx].active_workspace;
        self.focused_workspace = workspace_idx;

        // Focus a window on that workspace if any, or warp to monitor center
        if let Some(window) = self.workspaces[workspace_idx].focused
            .or_else(|| self.workspaces[workspace_idx].floating.last().copied())
            .or_else(|| self.workspaces[workspace_idx].tree.first_window())
        {
            // set_focus handles grab/ungrab for old and new windows
            self.set_focus(window, true)?;
        } else {
            // No windows - warp to monitor center
            self.focused_window = None;
            self.warp_to_monitor(target_idx)?;
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Move focused window to another monitor.
    /// Target can be "next", "prev", "left", "right", or a monitor name.
    fn move_to_monitor(&mut self, target: &str) -> Result<()> {
        if self.monitors.len() <= 1 {
            return Ok(());
        }

        let Some(window) = self.focused_window else {
            return Ok(());
        };

        let target_idx = match target.to_lowercase().as_str() {
            "next" | "right" => (self.focused_monitor + 1) % self.monitors.len(),
            "prev" | "left" => {
                if self.focused_monitor == 0 {
                    self.monitors.len() - 1
                } else {
                    self.focused_monitor - 1
                }
            }
            name => {
                match self.monitors.iter().position(|m| m.name.eq_ignore_ascii_case(name)) {
                    Some(idx) => idx,
                    None => {
                        tracing::warn!("Monitor '{}' not found", name);
                        return Ok(());
                    }
                }
            }
        };

        if target_idx == self.focused_monitor {
            return Ok(());
        }

        let is_floating = self.windows.get(&window).map(|w| w.floating).unwrap_or(false);
        let target_workspace = self.monitors[target_idx].active_workspace;

        tracing::info!("Moving window {} to monitor {}: '{}' (workspace {})",
            window, target_idx, self.monitors[target_idx].name, target_workspace + 1);

        // Remove from current workspace
        if is_floating {
            self.current_workspace_mut().remove_floating(window);
        } else {
            self.current_workspace_mut().tree.remove(window);
        }

        // Update window's workspace
        if let Some(win) = self.windows.get_mut(&window) {
            win.workspace = target_workspace;
        }

        // Add to target workspace
        if is_floating {
            self.workspaces[target_workspace].add_floating(window);
        } else {
            let target_focused = self.workspaces[target_workspace].focused;
            let target_rect = self.monitors[target_idx].geometry;
            self.workspaces[target_workspace].tree.insert_with_rect(window, target_focused, target_rect);
        }

        // Update EWMH
        self.conn.set_window_desktop(window, target_workspace as u32)?;

        // Focus follows window to new monitor
        self.focused_monitor = target_idx;
        self.focused_workspace = target_workspace;
        self.workspaces[target_workspace].focused = Some(window);

        // Apply layouts on both monitors
        self.apply_layout()?;

        // Set X11 focus on the moved window (updates focused_window, button grabs, EWMH)
        self.set_focus(window, true)?;

        self.conn.flush()?;
        Ok(())
    }

    /// Cycle through floating windows on the current workspace.
    fn cycle_floating(&mut self) -> Result<()> {
        let floating = &self.current_workspace().floating;
        if floating.is_empty() {
            tracing::debug!("No floating windows to cycle");
            return Ok(());
        }

        // Find current position in floating list
        let current_idx = self.focused_window
            .and_then(|w| floating.iter().position(|&fw| fw == w));

        // Get next floating window (wrap around)
        let next_idx = match current_idx {
            Some(idx) => (idx + 1) % floating.len(),
            None => 0, // Not focused on a floating window, focus the first one
        };

        let next_window = floating[next_idx];

        // Focus and raise the next floating window (keyboard action, warp pointer)
        // set_focus handles grab/ungrab for old and new windows
        self.set_focus(next_window, true)?;
        self.raise_window(next_window)?;

        tracing::debug!("Cycled to floating window {} (idx {})", next_window, next_idx);
        Ok(())
    }

    fn toggle_floating(&mut self, window: u32) -> Result<()> {
        // Check if window is managed
        let Some(win_state) = self.windows.get(&window) else {
            tracing::warn!("toggle_floating: window {} not managed", window);
            return Ok(());
        };
        let is_floating = win_state.floating;

        tracing::debug!(
            "toggle_floating: window={}, is_floating={}, in_tree={}, in_floating_list={}",
            window,
            is_floating,
            self.current_workspace().tree.contains(window),
            self.current_workspace().floating.contains(&window)
        );

        if is_floating {
            // Return to tiled
            tracing::info!("Returning window {} to tiled", window);

            // Update window state
            if let Some(win) = self.windows.get_mut(&window) {
                win.floating = false;
            }

            // Event mask for tiled window (includes POINTER_MOTION for tiled edge resize)
            self.conn.select_input(
                window,
                EventMask::ENTER_WINDOW
                    | EventMask::FOCUS_CHANGE
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::POINTER_MOTION,
            )?;

            // Clear edge cursor state if this window had one
            if self.current_edge_cursor.map(|(w, _)| w) == Some(window) {
                self.conn.clear_window_cursor(window)?;
                self.current_edge_cursor = None;
            }

            // Remove from floating list
            self.current_workspace_mut().remove_floating(window);

            // Find a target window to insert next to (not ourselves)
            let target = self.current_workspace().tree.first_window();
            let screen = self.screen_rect();
            self.current_workspace_mut()
                .tree
                .insert_with_rect(window, target, screen);

            // Re-apply layout
            self.apply_layout()?;
        } else {
            // Make floating
            tracing::info!("Floating window {}", window);

            // Check if window is actually in the tree
            if !self.current_workspace().tree.contains(window) {
                tracing::warn!("toggle_floating: window {} not in tree, cannot float", window);
                return Ok(());
            }

            // Use a centered floating geometry (80% of screen size, centered)
            let screen = self.screen_rect();
            let float_w = (screen.width * 4 / 5).max(400);
            let float_h = (screen.height * 4 / 5).max(300);
            let float_x = screen.x + (screen.width as i16 - float_w as i16) / 2;
            let float_y = screen.y + (screen.height as i16 - float_h as i16) / 2;
            let geometry = Rect::new(float_x, float_y, float_w, float_h);

            tracing::debug!("Floating geometry: {:?}", geometry);

            // Remove from BSP tree
            let removed = self.current_workspace_mut().tree.remove(window);
            tracing::debug!("Removed from tree: {}", removed);

            // Update window state with floating geometry
            if let Some(win) = self.windows.get_mut(&window) {
                win.floating = true;
                win.floating_geometry = geometry;
            }

            // Event mask for floating window (includes POINTER_MOTION for edge cursor)
            self.conn.select_input(
                window,
                EventMask::ENTER_WINDOW
                    | EventMask::FOCUS_CHANGE
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::POINTER_MOTION,
            )?;

            // Don't grab buttons - the window is already focused (we're acting on focused window)
            // and grabbing would intercept all clicks, preventing app interaction.
            // Mod+button grabs on root handle floating move/resize.
            // Button grabs are only for click-to-focus on unfocused windows.

            // Add to floating list (on top)
            self.current_workspace_mut().add_floating(window);

            // Re-apply layout (this will configure the floating window and stack it)
            self.apply_layout()?;
        }

        Ok(())
    }

    // =========================================================================
    // i3-compatible IPC handling (for polybar integration)
    // =========================================================================

    /// Handle pending i3-compatible IPC requests
    fn handle_i3_ipc(&mut self) -> Result<()> {
        use crate::ipc::i3_compat::MessageType;
        use crate::ipc::i3_server::{
            build_workspaces_json, build_outputs_json, build_version_json,
            build_subscribe_success_json,
        };

        let Some(ref mut i3_ipc) = self.i3_ipc_server else {
            return Ok(());
        };

        // Accept new connections
        i3_ipc.accept_connections();

        // Process requests
        let requests = i3_ipc.poll_requests();
        for (client_idx, msg) in requests {
            let msg_type = msg.msg_type;

            match MessageType::from_u32(msg_type) {
                Some(MessageType::GetWorkspaces) => {
                    let workspaces = self.build_i3_workspaces();
                    let focused_ws: Vec<_> = workspaces.iter().filter(|w| w.focused).map(|w| &w.name).collect();
                    tracing::debug!("GET_WORKSPACES: returning {} workspaces, focused: {:?}", workspaces.len(), focused_ws);
                    let json = build_workspaces_json(&workspaces);
                    if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                        i3_ipc.send_response(client_idx, msg_type, &json);
                    }
                }
                Some(MessageType::GetOutputs) => {
                    let outputs = self.build_i3_outputs();
                    let json = build_outputs_json(&outputs);
                    if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                        i3_ipc.send_response(client_idx, msg_type, &json);
                    }
                }
                Some(MessageType::Subscribe) => {
                    // Parse subscription request
                    if let Ok(events) = msg.payload_str() {
                        if let Ok(event_list) = serde_json::from_str::<Vec<String>>(events) {
                            if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                                i3_ipc.subscribe(client_idx, event_list);
                            }
                        }
                    }
                    let json = build_subscribe_success_json();
                    if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                        i3_ipc.send_response(client_idx, msg_type, &json);
                    }
                }
                Some(MessageType::GetVersion) => {
                    let json = build_version_json();
                    if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                        i3_ipc.send_response(client_idx, msg_type, &json);
                    }
                }
                Some(MessageType::RunCommand) => {
                    // Parse and execute i3-compatible commands
                    let cmd_str = msg.payload_str().unwrap_or("");
                    let success = self.execute_i3_command(cmd_str);
                    let json = if success {
                        r#"[{"success":true}]"#
                    } else {
                        r#"[{"success":false,"error":"command failed"}]"#
                    };
                    if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                        i3_ipc.send_response(client_idx, msg_type, json);
                    }
                }
                _ => {
                    // Unknown or unsupported message type - return empty success
                    let json = r#"{"success":false,"error":"unsupported"}"#;
                    if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                        i3_ipc.send_response(client_idx, msg_type, json);
                    }
                }
            }
        }

        // Clean up disconnected/stale clients AFTER processing requests
        // to avoid index invalidation during send_response
        if let Some(ref mut i3_ipc) = self.i3_ipc_server {
            i3_ipc.cleanup_clients();
        }

        Ok(())
    }

    /// Build i3-compatible workspace list
    /// Only includes workspaces that are visible or have windows (like i3)
    fn build_i3_workspaces(&self) -> Vec<crate::ipc::I3WorkspaceInfo> {
        use crate::ipc::{I3WorkspaceInfo, I3Rect};

        self.workspaces.iter().enumerate().filter_map(|(i, ws)| {
            // Find which monitor this workspace is on (if visible)
            let monitor = self.monitors.iter().find(|m| m.active_workspace == i);
            let visible = monitor.is_some();
            let has_windows = ws.has_windows();

            // Only include workspaces that are visible OR have windows
            if !visible && !has_windows {
                return None;
            }

            let focused = self.focused_monitor < self.monitors.len()
                && self.monitors[self.focused_monitor].active_workspace == i;

            // Check if any window in this workspace is urgent
            let urgent = self.windows.values()
                .filter(|w| w.workspace == i)
                .any(|w| w.urgent);

            // Get geometry and output from monitor
            // For non-visible workspaces with windows, assign to focused monitor
            let (rect, output) = if let Some(mon) = monitor {
                (
                    I3Rect {
                        x: mon.geometry.x as i32,
                        y: mon.geometry.y as i32,
                        width: mon.geometry.width as i32,
                        height: mon.geometry.height as i32,
                    },
                    mon.name.clone(),
                )
            } else {
                // Not visible but has windows - assign to focused monitor
                let mon = &self.monitors[self.focused_monitor];
                (
                    I3Rect {
                        x: mon.geometry.x as i32,
                        y: mon.geometry.y as i32,
                        width: mon.geometry.width as i32,
                        height: mon.geometry.height as i32,
                    },
                    mon.name.clone(),
                )
            };

            Some(I3WorkspaceInfo {
                id: (i + 1) as i64 * 1000000, // Generate unique ID
                num: (i + 1) as i32,
                name: ws.name.clone(),
                visible,
                focused,
                urgent,
                rect,
                output,
            })
        }).collect()
    }

    /// Build i3-compatible output list
    fn build_i3_outputs(&self) -> Vec<crate::ipc::OutputInfo> {
        use crate::ipc::{OutputInfo, I3Rect};

        self.monitors.iter().map(|mon| {
            let current_workspace = Some(self.workspaces[mon.active_workspace].name.clone());

            OutputInfo {
                name: mon.name.clone(),
                active: true,
                primary: mon.primary,
                current_workspace,
                rect: I3Rect {
                    x: mon.geometry.x as i32,
                    y: mon.geometry.y as i32,
                    width: mon.geometry.width as i32,
                    height: mon.geometry.height as i32,
                },
            }
        }).collect()
    }

    /// Broadcast i3 workspace event to subscribed clients
    pub fn broadcast_i3_workspace_event(&mut self, change: &str, workspace_idx: usize, old_workspace_idx: Option<usize>) {
        use crate::ipc::i3_server::build_workspace_event_json;

        let workspaces = self.build_i3_workspaces();

        // Find workspace by num (workspace_idx + 1), not by array index
        // build_i3_workspaces filters out empty/invisible workspaces so indices don't match
        let workspace_num = (workspace_idx + 1) as i32;
        let current = workspaces.iter().find(|w| w.num == workspace_num).cloned();
        let old = old_workspace_idx.and_then(|idx| {
            let old_num = (idx + 1) as i32;
            workspaces.iter().find(|w| w.num == old_num).cloned()
        });

        if let Some(current) = current {
            let json = build_workspace_event_json(change, &current, old.as_ref());
            tracing::debug!("Broadcasting i3 workspace event: change={}, workspace={}", change, current.num);
            if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                i3_ipc.broadcast_workspace_event(&json);
            }
        } else {
            tracing::warn!("Could not find workspace {} in i3 workspace list for broadcast", workspace_num);
        }
    }

    /// Broadcast i3 output event to subscribed clients
    pub fn broadcast_i3_output_event(&mut self) {
        use crate::ipc::i3_server::build_output_event_json;

        let json = build_output_event_json();
        if let Some(ref mut i3_ipc) = self.i3_ipc_server {
            i3_ipc.broadcast_output_event(&json);
        }
    }
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s.to_lowercase().as_str() {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

/// Threshold in pixels for detecting edge proximity
const EDGE_THRESHOLD: i16 = 12;

/// Determine which edge/corner of a window a point is near.
/// Returns ResizeEdge::None if not near any edge.
fn determine_resize_edge(geometry: &Rect, click_x: i16, click_y: i16) -> ResizeEdge {
    let left = geometry.x;
    let right = geometry.x + geometry.width as i16;
    let top = geometry.y;
    let bottom = geometry.y + geometry.height as i16;

    let near_left = click_x >= left && click_x < left + EDGE_THRESHOLD;
    let near_right = click_x > right - EDGE_THRESHOLD && click_x <= right;
    let near_top = click_y >= top && click_y < top + EDGE_THRESHOLD;
    let near_bottom = click_y > bottom - EDGE_THRESHOLD && click_y <= bottom;

    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => ResizeEdge::TopLeft,
        (_, true, true, _) => ResizeEdge::TopRight,
        (true, _, _, true) => ResizeEdge::BottomLeft,
        (_, true, _, true) => ResizeEdge::BottomRight,
        (true, _, _, _) => ResizeEdge::Left,
        (_, true, _, _) => ResizeEdge::Right,
        (_, _, true, _) => ResizeEdge::Top,
        (_, _, _, true) => ResizeEdge::Bottom,
        _ => ResizeEdge::None,
    }
}

/// Determine resize edge for mod+click (quadrant-based, always picks a corner)
fn determine_resize_edge_quadrant(geometry: &Rect, click_x: i16, click_y: i16) -> ResizeEdge {
    let center_x = geometry.x + geometry.width as i16 / 2;
    let center_y = geometry.y + geometry.height as i16 / 2;

    match (click_x < center_x, click_y < center_y) {
        (true, true) => ResizeEdge::TopLeft,
        (false, true) => ResizeEdge::TopRight,
        (true, false) => ResizeEdge::BottomLeft,
        (false, false) => ResizeEdge::BottomRight,
    }
}

fn calculate_resize(geometry: &Rect, edge: ResizeEdge, dx: i16, dy: i16) -> (i16, i16, u16, u16) {
    const MIN_SIZE: u16 = 50;

    let (mut x, mut y, mut w, mut h) = (
        geometry.x,
        geometry.y,
        geometry.width,
        geometry.height,
    );

    match edge {
        ResizeEdge::TopLeft => {
            x += dx;
            y += dy;
            w = (w as i16 - dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 - dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::Top => {
            y += dy;
            h = (h as i16 - dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::TopRight => {
            y += dy;
            w = (w as i16 + dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 - dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::Left => {
            x += dx;
            w = (w as i16 - dx).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::Right => {
            w = (w as i16 + dx).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::BottomLeft => {
            x += dx;
            w = (w as i16 - dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 + dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::Bottom => {
            h = (h as i16 + dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::BottomRight => {
            w = (w as i16 + dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 + dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::None => {
            // No resize
        }
    }

    (x, y, w, h)
}
