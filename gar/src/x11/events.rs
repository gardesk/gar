use std::process::Command;

use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    ButtonPressEvent, ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt,
    DestroyNotifyEvent, EventMask, KeyPressEvent, MapRequestEvent, ModMask, UnmapNotifyEvent,
};
use x11rb::protocol::Event;

use crate::core::WindowManager;
use crate::Result;

// XK_Return keysym
const XK_RETURN: u32 = 0xff0d;

impl WindowManager {
    /// Set up initial keybinds and grabs.
    pub fn setup_grabs(&mut self) -> Result<()> {
        // Grab Mod4 + Return for terminal
        if let Some(keycode) = self.conn.keycode_from_keysym(XK_RETURN) {
            self.conn.grab_key(ModMask::M4, keycode)?;
            tracing::info!("Grabbed Mod4+Return (keycode {})", keycode);
        } else {
            tracing::warn!("Could not find keycode for Return key");
        }

        self.conn.flush()?;
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
        // So we ignore the configure request (or could send a synthetic ConfigureNotify)

        Ok(())
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) -> Result<()> {
        tracing::debug!("UnmapNotify for window {}", event.window);

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
        let modifiers = event.state;
        tracing::debug!("KeyPress: keycode={}, modifiers={:?}", keycode, modifiers);

        // Check for Mod4 + Return
        let return_keycode = self.conn.keycode_from_keysym(XK_RETURN);
        if Some(keycode) == return_keycode && modifiers.contains(ModMask::M4) {
            tracing::info!("Spawning terminal");
            self.spawn_terminal();
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
