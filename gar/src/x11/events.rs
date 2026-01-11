use std::process::Command;

use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    ButtonPressEvent, ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt,
    DestroyNotifyEvent, EventMask, KeyPressEvent, MapRequestEvent, ModMask, UnmapNotifyEvent,
};
use x11rb::protocol::Event;

use crate::core::{Direction, Node, WindowManager};
use crate::Result;

// Keysym constants
const XK_RETURN: u32 = 0xff0d;
const XK_Q: u32 = 0x71;
const XK_E: u32 = 0x65;
const XK_LEFT: u32 = 0xff51;
const XK_UP: u32 = 0xff52;
const XK_RIGHT: u32 = 0xff53;
const XK_DOWN: u32 = 0xff54;
const XK_1: u32 = 0x31;
const XK_2: u32 = 0x32;
const XK_3: u32 = 0x33;
const XK_4: u32 = 0x34;
const XK_5: u32 = 0x35;
const XK_6: u32 = 0x36;
const XK_7: u32 = 0x37;
const XK_8: u32 = 0x38;
const XK_9: u32 = 0x39;
const XK_0: u32 = 0x30;

/// Keybind action types
#[derive(Debug, Clone)]
enum Action {
    SpawnTerminal,
    CloseWindow,
    Focus(Direction),
    Swap(Direction),
    Resize(Direction),
    Equalize,
    SwitchWorkspace(usize),
    MoveToWorkspace(usize),
}

struct Keybind {
    modifiers: ModMask,
    keysym: u32,
    action: Action,
}

impl WindowManager {
    /// Get all keybinds to register.
    /// NOTE: Using Alt (M1) instead of Super (M4) for testing in nested X
    fn keybinds() -> Vec<Keybind> {
        vec![
            // Alt+Return: spawn terminal
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_RETURN,
                action: Action::SpawnTerminal,
            },
            // Alt+Q: close window
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_Q,
                action: Action::CloseWindow,
            },
            // Alt+E: equalize splits
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_E,
                action: Action::Equalize,
            },
            // Alt+Arrows: focus navigation
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_LEFT,
                action: Action::Focus(Direction::Left),
            },
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_RIGHT,
                action: Action::Focus(Direction::Right),
            },
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_UP,
                action: Action::Focus(Direction::Up),
            },
            Keybind {
                modifiers: ModMask::M1,
                keysym: XK_DOWN,
                action: Action::Focus(Direction::Down),
            },
            // Alt+Shift+Arrows: swap windows
            Keybind {
                modifiers: ModMask::M1 | ModMask::SHIFT,
                keysym: XK_LEFT,
                action: Action::Swap(Direction::Left),
            },
            Keybind {
                modifiers: ModMask::M1 | ModMask::SHIFT,
                keysym: XK_RIGHT,
                action: Action::Swap(Direction::Right),
            },
            Keybind {
                modifiers: ModMask::M1 | ModMask::SHIFT,
                keysym: XK_UP,
                action: Action::Swap(Direction::Up),
            },
            Keybind {
                modifiers: ModMask::M1 | ModMask::SHIFT,
                keysym: XK_DOWN,
                action: Action::Swap(Direction::Down),
            },
            // Alt+Ctrl+Arrows: resize
            Keybind {
                modifiers: ModMask::M1 | ModMask::CONTROL,
                keysym: XK_LEFT,
                action: Action::Resize(Direction::Left),
            },
            Keybind {
                modifiers: ModMask::M1 | ModMask::CONTROL,
                keysym: XK_RIGHT,
                action: Action::Resize(Direction::Right),
            },
            Keybind {
                modifiers: ModMask::M1 | ModMask::CONTROL,
                keysym: XK_UP,
                action: Action::Resize(Direction::Up),
            },
            Keybind {
                modifiers: ModMask::M1 | ModMask::CONTROL,
                keysym: XK_DOWN,
                action: Action::Resize(Direction::Down),
            },
            // Alt+1-9,0: switch workspace
            Keybind { modifiers: ModMask::M1, keysym: XK_1, action: Action::SwitchWorkspace(0) },
            Keybind { modifiers: ModMask::M1, keysym: XK_2, action: Action::SwitchWorkspace(1) },
            Keybind { modifiers: ModMask::M1, keysym: XK_3, action: Action::SwitchWorkspace(2) },
            Keybind { modifiers: ModMask::M1, keysym: XK_4, action: Action::SwitchWorkspace(3) },
            Keybind { modifiers: ModMask::M1, keysym: XK_5, action: Action::SwitchWorkspace(4) },
            Keybind { modifiers: ModMask::M1, keysym: XK_6, action: Action::SwitchWorkspace(5) },
            Keybind { modifiers: ModMask::M1, keysym: XK_7, action: Action::SwitchWorkspace(6) },
            Keybind { modifiers: ModMask::M1, keysym: XK_8, action: Action::SwitchWorkspace(7) },
            Keybind { modifiers: ModMask::M1, keysym: XK_9, action: Action::SwitchWorkspace(8) },
            Keybind { modifiers: ModMask::M1, keysym: XK_0, action: Action::SwitchWorkspace(9) },
            // Alt+Shift+1-9,0: move window to workspace
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_1, action: Action::MoveToWorkspace(0) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_2, action: Action::MoveToWorkspace(1) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_3, action: Action::MoveToWorkspace(2) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_4, action: Action::MoveToWorkspace(3) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_5, action: Action::MoveToWorkspace(4) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_6, action: Action::MoveToWorkspace(5) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_7, action: Action::MoveToWorkspace(6) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_8, action: Action::MoveToWorkspace(7) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_9, action: Action::MoveToWorkspace(8) },
            Keybind { modifiers: ModMask::M1 | ModMask::SHIFT, keysym: XK_0, action: Action::MoveToWorkspace(9) },
        ]
    }

    /// Set up initial keybinds and grabs.
    pub fn setup_grabs(&mut self) -> Result<()> {
        for keybind in Self::keybinds() {
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
        tracing::info!("Keybinds registered");
        Ok(())
    }

    pub fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::MapRequest(e) => self.handle_map_request(e)?,
            Event::ConfigureRequest(e) => self.handle_configure_request(e)?,
            Event::UnmapNotify(e) => self.handle_unmap_notify(e)?,
            Event::DestroyNotify(e) => self.handle_destroy_notify(e)?,
            Event::ButtonPress(e) => self.handle_button_press(e)?,
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
        tracing::debug!("ButtonPress on window {}", window);

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

            // Replay the click so the application receives it
            self.conn.conn.allow_events(
                x11rb::protocol::xproto::Allow::REPLAY_POINTER,
                x11rb::CURRENT_TIME,
            )?;
        }

        self.conn.flush()?;
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

        // Find matching keybind
        for keybind in Self::keybinds() {
            let bind_keycode = self.conn.keycode_from_keysym(keybind.keysym);
            if bind_keycode == Some(keycode) && keybind.modifiers == modifiers {
                tracing::debug!("Executing action: {:?}", keybind.action);
                self.execute_action(keybind.action)?;
                return Ok(());
            }
        }

        Ok(())
    }

    fn execute_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::SpawnTerminal => {
                self.spawn_terminal();
            }
            Action::CloseWindow => {
                if let Some(window) = self.focused_window {
                    self.close_window(window)?;
                }
            }
            Action::Focus(direction) => {
                self.focus_direction(direction)?;
            }
            Action::Swap(direction) => {
                self.swap_direction(direction)?;
            }
            Action::Resize(direction) => {
                self.resize_direction(direction)?;
            }
            Action::Equalize => {
                self.equalize()?;
            }
            Action::SwitchWorkspace(idx) => {
                self.switch_workspace(idx)?;
            }
            Action::MoveToWorkspace(idx) => {
                self.move_to_workspace(idx)?;
            }
        }
        Ok(())
    }

    fn spawn_terminal(&self) {
        // Try common terminals in order of preference
        let terminals = ["alacritty", "kitty", "foot", "xterm"];

        for terminal in terminals {
            match Command::new(terminal).spawn() {
                Ok(_) => {
                    tracing::info!("Spawned {}", terminal);
                    return;
                }
                Err(_) => continue,
            }
        }

        tracing::warn!("No terminal emulator found");
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

    fn resize_direction(&mut self, direction: Direction) -> Result<()> {
        let Some(focused) = self.focused_window else {
            return Ok(());
        };

        const RESIZE_DELTA: f32 = 0.05;

        // Resize the split
        if self
            .current_workspace_mut()
            .tree
            .resize(focused, direction, RESIZE_DELTA)
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
        if let Some(window) = self.current_workspace().focused.or_else(|| self.current_workspace().tree.first_window()) {
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
        self.workspaces[idx].tree.insert_with_rect(window, target_focused, screen);

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
}
