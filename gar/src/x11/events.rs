use std::process::Command;

use x11rb::connection::Connection as X11Connection;
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
}

#[derive(Debug, Clone, Copy)]
pub enum ResizeEdge {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
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

            // Subscribe to events on the window
            self.conn.select_input(
                window,
                EventMask::ENTER_WINDOW
                    | EventMask::FOCUS_CHANGE
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::STRUCTURE_NOTIFY,
            )?;

            // Grab button for click-to-focus
            self.conn.grab_button(window)?;

            // Check window rules and EWMH hints
            let rule_actions = self.check_rules(window);
            let should_float = rule_actions.floating.unwrap_or_else(|| self.conn.should_float(window));

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
                self.conn.ungrab_button(window)?;
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

        // Subscribe to events on the window
        self.conn.select_input(
            window,
            EventMask::ENTER_WINDOW
                | EventMask::FOCUS_CHANGE
                | EventMask::PROPERTY_CHANGE
                | EventMask::STRUCTURE_NOTIFY,
        )?;

        // Grab button for click-to-focus
        self.conn.grab_button(window)?;

        // Check window rules first
        let rule_actions = self.check_rules(window);

        // Determine target workspace (rule or current)
        let target_workspace = rule_actions.workspace.unwrap_or(self.focused_workspace + 1);
        let target_idx = target_workspace.saturating_sub(1).min(self.workspaces.len() - 1);

        // Determine if window should float (rule > ICCCM/EWMH hints)
        let should_float = rule_actions.floating.unwrap_or_else(|| self.conn.should_float(window));

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
        tracing::debug!("UnmapNotify for window {}", window);

        // Check if this was a dock window with struts
        if self.dock_struts.remove(&window).is_some() {
            tracing::info!("Dock window {} unmapped, removing strut", window);
            self.apply_layout()?;
            self.conn.flush()?;
            return Ok(());
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

        // Check for mod+click on floating windows (move/resize)
        let has_mod = event.state.contains(x11rb::protocol::xproto::KeyButMask::MOD1);

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
                // Grab pointer for motion events
                self.conn.grab_pointer(target)?;
                return Ok(());
            } else if event.detail == 3 {
                // Mod+Button3 = Resize
                let edge = determine_resize_edge(&geometry, event.root_x, event.root_y);
                tracing::debug!("Starting resize for floating window {}, edge {:?}", target, edge);
                self.drag_state = Some(DragState::Resize {
                    window: target,
                    start_x: event.root_x,
                    start_y: event.root_y,
                    start_geometry: geometry,
                    edge,
                });
                self.conn.grab_pointer(target)?;
                return Ok(());
            }
        }

        // Only handle if we manage this window
        if !self.windows.contains_key(&window) {
            return Ok(());
        }

        // Focus the clicked window
        if self.focused_window != Some(window) {
            // Regrab button on old focused window
            if let Some(old) = self.focused_window {
                self.conn.grab_button(old)?;
            }

            // Set focus and ungrab button on new focused window (no warp - mouse click)
            self.set_focus(window, false)?;
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

    fn handle_button_release(&mut self, _event: ButtonReleaseEvent) -> Result<()> {
        if self.drag_state.is_some() {
            tracing::debug!("Ending drag operation");
            self.drag_state = None;
            self.conn.ungrab_pointer()?;
            self.conn.flush()?;
        }
        Ok(())
    }

    fn handle_motion_notify(&mut self, event: MotionNotifyEvent) -> Result<()> {
        let Some(ref drag) = self.drag_state else {
            return Ok(());
        };

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
        }

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

        // Regrab button on old focused window
        if let Some(old) = self.focused_window {
            self.conn.grab_button(old)?;
        }

        // Focus the new window (no warp - mouse enter)
        self.set_focus(window, false)?;
        self.conn.ungrab_button(window)?;

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
            Action::MoveToWorkspace(idx) => {
                // Lua uses 1-based indexing
                self.move_to_workspace(idx.saturating_sub(1))?;
            }
            Action::Reload => {
                self.reload_config()?;
            }
            Action::Exit => {
                tracing::info!("Exit requested");
                self.running = false;
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

        if let Some(target) = Node::find_adjacent(&geometries, focused, direction) {
            // Regrab button on old window
            self.conn.grab_button(focused)?;

            // Focus new window (keyboard navigation, warp pointer)
            self.set_focus(target, true)?;
            self.conn.ungrab_button(target)?;
            self.conn.flush()?;

            tracing::debug!("Focused {:?} to window {}", direction, target);
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
            if let Some(old) = self.focused_window {
                self.conn.grab_button(old)?;
            }
            self.set_focus(window, true)?;
            self.conn.ungrab_button(window)?;
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

        if let Some(target) = Node::find_adjacent(&geometries, focused, direction) {
            // Swap the windows in the tree
            self.current_workspace_mut().tree.swap(focused, target);

            // Re-apply layout
            self.apply_layout()?;

            tracing::debug!("Swapped with window {} in direction {:?}", target, direction);
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

    /// Switch to workspace using i3-style behavior:
    /// - If workspace is visible on another monitor, focus moves to that monitor
    /// - If workspace is not visible, it appears on the current monitor
    fn switch_workspace(&mut self, idx: usize) -> Result<()> {
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

            // Warp pointer to that monitor
            let monitor_geom = self.monitors[monitor_idx].geometry;
            let center_x = monitor_geom.x + (monitor_geom.width as i16 / 2);
            let center_y = monitor_geom.y + (monitor_geom.height as i16 / 2);
            self.conn.warp_pointer(center_x, center_y)?;
            self.last_warp = std::time::Instant::now();

            // Focus a window on that workspace
            if let Some(window) = self.workspaces[idx].focused
                .or_else(|| self.workspaces[idx].floating.last().copied())
                .or_else(|| self.workspaces[idx].tree.first_window())
            {
                self.set_focus(window, true)?;
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
                self.set_focus(window, true)?;
            } else {
                // No windows - warp to center of monitor
                self.focused_window = None;
                self.conn.set_active_window(None)?;
                let monitor_geom = self.monitors[current_monitor].geometry;
                let center_x = monitor_geom.x + (monitor_geom.width as i16 / 2);
                let center_y = monitor_geom.y + (monitor_geom.height as i16 / 2);
                self.conn.warp_pointer(center_x, center_y)?;
                self.last_warp = std::time::Instant::now();
            }
        }

        // Broadcast i3 workspace event for polybar
        if idx != old_workspace_idx {
            self.broadcast_i3_workspace_event("focus", idx, Some(old_workspace_idx));
        }

        self.conn.flush()?;
        Ok(())
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

        // If target workspace is visible, map the window; otherwise hide it
        if target_visible_on.is_some() {
            // Target is visible - map the window
            if let Some(frame) = self.frames.frame_for_client(window) {
                self.conn.map_window(frame)?;
            }
            self.conn.map_window(window)?;
        } else {
            // Target is not visible - hide the window
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

        // Re-register keybinds
        self.setup_grabs()?;

        // Re-apply layout with new settings
        self.apply_layout()?;

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

        while self.running {
            // Handle X11 events (non-blocking poll)
            while let Some(event) = self.conn.conn.poll_for_event()? {
                self.handle_event(event)?;
            }

            // Handle IPC requests
            self.handle_ipc()?;

            // Handle i3-compatible IPC requests (for polybar)
            self.handle_i3_ipc()?;

            // Small sleep to avoid busy-waiting when idle
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

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
            if let Some(old) = self.focused_window {
                self.conn.grab_button(old)?;
            }
            self.set_focus(window, true)?;
            self.conn.ungrab_button(window)?;
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

        // Regrab button on old focused window
        if let Some(old) = self.focused_window {
            self.conn.grab_button(old)?;
        }

        // Focus and raise the next floating window (keyboard action, warp pointer)
        self.set_focus(next_window, true)?;
        self.conn.ungrab_button(next_window)?;
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
                    // Return success for now - we could parse and execute i3 commands later
                    let json = r#"[{"success":true}]"#;
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

        Ok(())
    }

    /// Build i3-compatible workspace list
    fn build_i3_workspaces(&self) -> Vec<crate::ipc::I3WorkspaceInfo> {
        use crate::ipc::{I3WorkspaceInfo, I3Rect};

        self.workspaces.iter().enumerate().map(|(i, ws)| {
            // Find which monitor this workspace is on (if visible)
            let monitor = self.monitors.iter().find(|m| m.active_workspace == i);
            let visible = monitor.is_some();
            let focused = self.focused_monitor < self.monitors.len()
                && self.monitors[self.focused_monitor].active_workspace == i;

            // Check if any window in this workspace is urgent
            let urgent = self.windows.values()
                .filter(|w| w.workspace == i)
                .any(|w| w.urgent);

            // Get geometry from monitor if visible, else use first monitor's geometry
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
                // Not visible - use first monitor as fallback
                let fallback = &self.monitors[0];
                (
                    I3Rect {
                        x: fallback.geometry.x as i32,
                        y: fallback.geometry.y as i32,
                        width: fallback.geometry.width as i32,
                        height: fallback.geometry.height as i32,
                    },
                    fallback.name.clone(),
                )
            };

            I3WorkspaceInfo {
                id: (i + 1) as i64 * 1000000, // Generate unique ID
                num: (i + 1) as i32,
                name: ws.name.clone(),
                visible,
                focused,
                urgent,
                rect,
                output,
            }
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
        let current = workspaces.get(workspace_idx).cloned();
        let old = old_workspace_idx.and_then(|idx| workspaces.get(idx).cloned());

        if let Some(current) = current {
            let json = build_workspace_event_json(change, &current, old.as_ref());
            if let Some(ref mut i3_ipc) = self.i3_ipc_server {
                i3_ipc.broadcast_workspace_event(&json);
            }
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

fn determine_resize_edge(geometry: &Rect, click_x: i16, click_y: i16) -> ResizeEdge {
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
        ResizeEdge::TopRight => {
            y += dy;
            w = (w as i16 + dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 - dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::BottomLeft => {
            x += dx;
            w = (w as i16 - dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 + dy).max(MIN_SIZE as i16) as u16;
        }
        ResizeEdge::BottomRight => {
            w = (w as i16 + dx).max(MIN_SIZE as i16) as u16;
            h = (h as i16 + dy).max(MIN_SIZE as i16) as u16;
        }
    }

    (x, y, w, h)
}
