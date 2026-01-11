use std::process::Command;

use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    ButtonPressEvent, ButtonReleaseEvent, ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt,
    DestroyNotifyEvent, EventMask, KeyPressEvent, MapRequestEvent, ModMask, MotionNotifyEvent,
    StackMode, UnmapNotifyEvent,
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

        self.conn.flush()?;
        tracing::info!("{} keybinds registered", state.keybinds.len());
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
                tracing::trace!("EnterNotify for window {}", e.event);
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

        // Manage the window
        self.manage_window(window);

        // Map the window
        self.conn.map_window(window)?;

        // Apply layout to all windows
        self.apply_layout()?;

        // Focus the new window
        self.set_focus(window)?;

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
        tracing::debug!("UnmapNotify for window {}", event.window);

        // Only unmanage if window is on current workspace
        // (windows on other workspaces are unmapped due to workspace switching)
        let on_current = self
            .windows
            .get(&event.window)
            .map(|w| w.workspace == self.focused_workspace)
            .unwrap_or(false);

        if on_current {
            // Remove from management
            self.unmanage_window(event.window);

            // Re-apply layout
            self.apply_layout()?;

            // Update focus
            if let Some(window) = self.focused_window {
                self.set_focus(window)?;
            }
        }

        Ok(())
    }

    fn handle_destroy_notify(&mut self, event: DestroyNotifyEvent) -> Result<()> {
        tracing::debug!("DestroyNotify for window {}", event.window);

        // Remove from management
        self.unmanage_window(event.window);

        // Re-apply layout
        self.apply_layout()?;

        // Update focus
        if let Some(window) = self.focused_window {
            self.set_focus(window)?;
        }

        Ok(())
    }

    fn handle_button_press(&mut self, event: ButtonPressEvent) -> Result<()> {
        let window = event.event;
        tracing::debug!("ButtonPress on window {}, button {}", window, event.detail);

        // Check for mod+click on floating windows (move/resize)
        let has_mod = event.state.contains(x11rb::protocol::xproto::KeyButMask::MOD1);

        if has_mod && self.is_floating(window) {
            let geometry = self.get_floating_geometry(window);

            if event.detail == 1 {
                // Mod+Button1 = Move
                tracing::debug!("Starting move for floating window {}", window);
                self.drag_state = Some(DragState::Move {
                    window,
                    start_x: event.root_x,
                    start_y: event.root_y,
                    start_geometry: geometry,
                });
                // Grab pointer for motion events
                self.conn.grab_pointer(window)?;
                return Ok(());
            } else if event.detail == 3 {
                // Mod+Button3 = Resize
                let edge = determine_resize_edge(&geometry, event.root_x, event.root_y);
                tracing::debug!("Starting resize for floating window {}, edge {:?}", window, edge);
                self.drag_state = Some(DragState::Resize {
                    window,
                    start_x: event.root_x,
                    start_y: event.root_y,
                    start_geometry: geometry,
                    edge,
                });
                self.conn.grab_pointer(window)?;
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

            // Set focus and ungrab button on new focused window
            self.set_focus(window)?;
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

    fn handle_key_press(&mut self, event: KeyPressEvent) -> Result<()> {
        let keycode = event.detail;
        let state = event.state;

        // Convert KeyButMask to ModMask for comparison
        let modifiers = ModMask::from(
            (state.bits() & (ModMask::SHIFT | ModMask::CONTROL | ModMask::M1 | ModMask::M4).bits())
                as u16,
        );

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
            Action::LuaCallback(index) => {
                if let Err(e) = self.lua_config.execute_callback(index) {
                    tracing::error!("Lua callback error: {}", e);
                }
            }
        }
        Ok(())
    }

    fn close_window(&mut self, window: u32) -> Result<()> {
        tracing::info!("Closing window {}", window);

        // TODO: Send WM_DELETE_WINDOW if supported (ICCCM)
        // For now, just kill the client
        self.conn.conn.kill_client(window)?;
        self.conn.flush()?;

        Ok(())
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        let Some(focused) = self.focused_window else {
            return Ok(());
        };

        let screen = self.screen_rect();
        let geometries = self.current_workspace().tree.calculate_geometries(screen);

        if let Some(target) = Node::find_adjacent(&geometries, focused, direction) {
            // Regrab button on old window
            self.conn.grab_button(focused)?;

            // Focus new window
            self.set_focus(target)?;
            self.conn.ungrab_button(target)?;
            self.conn.flush()?;

            tracing::debug!("Focused {:?} to window {}", direction, target);
        }

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

    fn switch_workspace(&mut self, idx: usize) -> Result<()> {
        if idx >= self.workspaces.len() || idx == self.focused_workspace {
            return Ok(());
        }

        tracing::info!("Switching to workspace {}", idx + 1);

        // Hide windows on current workspace
        for window in self.current_workspace().tree.windows() {
            self.conn.unmap_window(window)?;
        }

        // Switch workspace
        self.focused_workspace = idx;

        // Show windows on new workspace
        for window in self.current_workspace().tree.windows() {
            self.conn.map_window(window)?;
        }

        // Apply layout and update focus
        self.apply_layout()?;

        // Focus the workspace's focused window or first window
        if let Some(window) = self
            .current_workspace()
            .focused
            .or_else(|| self.current_workspace().tree.first_window())
        {
            self.set_focus(window)?;
            self.conn.ungrab_button(window)?;
        } else {
            self.focused_window = None;
        }

        self.conn.flush()?;
        Ok(())
    }

    fn move_to_workspace(&mut self, idx: usize) -> Result<()> {
        if idx >= self.workspaces.len() || idx == self.focused_workspace {
            return Ok(());
        }

        let Some(window) = self.focused_window else {
            return Ok(());
        };

        tracing::info!("Moving window {} to workspace {}", window, idx + 1);

        // Remove from current workspace tree
        self.current_workspace_mut().tree.remove(window);

        // Update focus on current workspace
        self.focused_window = self.current_workspace().tree.first_window();
        self.current_workspace_mut().focused = self.focused_window;

        // Update window's workspace tracking
        if let Some(win) = self.windows.get_mut(&window) {
            win.workspace = idx;
        }

        // Hide the window (it's moving to another workspace)
        self.conn.unmap_window(window)?;

        // Insert into target workspace
        let target_focused = self.workspaces[idx].focused;
        let screen = self.screen_rect();
        self.workspaces[idx]
            .tree
            .insert_with_rect(window, target_focused, screen);

        // Re-apply layout on current workspace
        self.apply_layout()?;

        // Update focus
        if let Some(new_focus) = self.focused_window {
            self.set_focus(new_focus)?;
            self.conn.ungrab_button(new_focus)?;
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

        while self.running {
            let event = self.conn.conn.wait_for_event()?;
            self.handle_event(event)?;
        }

        tracing::info!("Event loop exited");
        Ok(())
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
            .map(|w| w.geometry)
            .unwrap_or_default()
    }

    fn set_floating_position(&mut self, window: u32, x: i16, y: i16) -> Result<()> {
        if let Some(win) = self.windows.get_mut(&window) {
            win.geometry.x = x;
            win.geometry.y = y;
            self.conn.configure_window(
                window,
                x,
                y,
                win.geometry.width,
                win.geometry.height,
                self.config.border_width,
            )?;
            self.conn.flush()?;
        }
        Ok(())
    }

    fn set_floating_geometry(&mut self, window: u32, x: i16, y: i16, w: u16, h: u16) -> Result<()> {
        if let Some(win) = self.windows.get_mut(&window) {
            win.geometry = Rect::new(x, y, w, h);
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
        let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        self.conn.conn.configure_window(window, &aux)?;
        self.conn.flush()?;
        Ok(())
    }

    fn toggle_floating(&mut self, window: u32) -> Result<()> {
        let Some(win) = self.windows.get_mut(&window) else {
            return Ok(());
        };

        if win.floating {
            // Return to tiled
            tracing::info!("Returning window {} to tiled", window);
            win.floating = false;

            // Remove from floating list
            self.current_workspace_mut()
                .floating
                .retain(|w| w.id != window);

            // Insert back into BSP tree
            let focused = self.current_workspace().focused;
            let screen = self.screen_rect();
            self.current_workspace_mut()
                .tree
                .insert_with_rect(window, focused, screen);

            // Re-apply layout
            self.apply_layout()?;
        } else {
            // Make floating
            tracing::info!("Floating window {}", window);

            // Get current geometry before removing from tree
            let screen = self.screen_rect();
            let geometries = self.current_workspace().tree.calculate_geometries(screen);
            let geometry = geometries
                .iter()
                .find(|(w, _)| *w == window)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| Rect::new(100, 100, 640, 480));

            // Remove from BSP tree
            self.current_workspace_mut().tree.remove(window);

            // Mark as floating and store geometry
            let win = self.windows.get_mut(&window).unwrap();
            win.floating = true;
            win.geometry = geometry;

            // Add to floating list
            let workspace_idx = self.focused_workspace;
            self.current_workspace_mut()
                .floating
                .push(crate::core::Window::new(window, workspace_idx));

            // Re-apply layout for tiled windows
            self.apply_layout()?;

            // Configure floating window to its geometry
            self.conn.configure_window(
                window,
                geometry.x,
                geometry.y,
                geometry.width,
                geometry.height,
                self.config.border_width,
            )?;

            // Raise floating window above tiled
            self.raise_window(window)?;
        }

        Ok(())
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
