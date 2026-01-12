use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ButtonIndex, ChangeWindowAttributesAux, ClientMessageData,
    ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, GrabMode, InputFocus,
    ModMask, Screen, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;
use x11rb::CURRENT_TIME;

use super::Error;

pub struct Connection {
    pub conn: RustConnection,
    pub screen_num: usize,
    pub root: Window,
    pub screen_width: u16,
    pub screen_height: u16,
    // ICCCM atoms
    pub wm_protocols: Atom,
    pub wm_delete_window: Atom,
    pub wm_transient_for: Atom,
    // EWMH atoms for window types
    pub net_wm_window_type: Atom,
    pub net_wm_window_type_dialog: Atom,
    pub net_wm_window_type_utility: Atom,
    pub net_wm_window_type_toolbar: Atom,
    pub net_wm_window_type_splash: Atom,
    pub net_wm_window_type_notification: Atom,
    // EWMH atoms for window state
    pub net_wm_state: Atom,
    pub net_wm_state_modal: Atom,
    // EWMH atoms for workspaces
    pub net_supported: Atom,
    pub net_number_of_desktops: Atom,
    pub net_current_desktop: Atom,
    pub net_desktop_names: Atom,
    pub net_wm_desktop: Atom,
    pub net_active_window: Atom,
    pub utf8_string: Atom,
}

impl Connection {
    pub fn new() -> Result<Self, Error> {
        let (conn, screen_num) = x11rb::connect(None)?;

        let screen = conn
            .setup()
            .roots
            .get(screen_num)
            .ok_or(Error::NoScreens)?;

        let root = screen.root;
        let screen_width = screen.width_in_pixels;
        let screen_height = screen.height_in_pixels;

        // Intern ICCCM atoms
        let wm_protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
        let wm_delete_window = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
        let wm_transient_for = conn.intern_atom(false, b"WM_TRANSIENT_FOR")?.reply()?.atom;

        // Intern EWMH atoms for window types
        let net_wm_window_type = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?.reply()?.atom;
        let net_wm_window_type_dialog = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DIALOG")?.reply()?.atom;
        let net_wm_window_type_utility = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY")?.reply()?.atom;
        let net_wm_window_type_toolbar = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_TOOLBAR")?.reply()?.atom;
        let net_wm_window_type_splash = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_SPLASH")?.reply()?.atom;
        let net_wm_window_type_notification = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_NOTIFICATION")?.reply()?.atom;

        // Intern EWMH atoms for window state
        let net_wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
        let net_wm_state_modal = conn.intern_atom(false, b"_NET_WM_STATE_MODAL")?.reply()?.atom;

        // Intern EWMH atoms for workspaces
        let net_supported = conn.intern_atom(false, b"_NET_SUPPORTED")?.reply()?.atom;
        let net_number_of_desktops = conn.intern_atom(false, b"_NET_NUMBER_OF_DESKTOPS")?.reply()?.atom;
        let net_current_desktop = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?.reply()?.atom;
        let net_desktop_names = conn.intern_atom(false, b"_NET_DESKTOP_NAMES")?.reply()?.atom;
        let net_wm_desktop = conn.intern_atom(false, b"_NET_WM_DESKTOP")?.reply()?.atom;
        let net_active_window = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
        let utf8_string = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

        tracing::info!(
            "Connected to X server, screen {}x{}",
            screen_width,
            screen_height
        );

        Ok(Self {
            conn,
            screen_num,
            root,
            screen_width,
            screen_height,
            wm_protocols,
            wm_delete_window,
            wm_transient_for,
            net_wm_window_type,
            net_wm_window_type_dialog,
            net_wm_window_type_utility,
            net_wm_window_type_toolbar,
            net_wm_window_type_splash,
            net_wm_window_type_notification,
            net_wm_state,
            net_wm_state_modal,
            net_supported,
            net_number_of_desktops,
            net_current_desktop,
            net_desktop_names,
            net_wm_desktop,
            net_active_window,
            utf8_string,
        })
    }

    pub fn screen(&self) -> &Screen {
        &self.conn.setup().roots[self.screen_num]
    }

    pub fn become_wm(&self) -> Result<(), Error> {
        let change = ChangeWindowAttributesAux::new().event_mask(
            EventMask::SUBSTRUCTURE_REDIRECT
                | EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::PROPERTY_CHANGE,
        );

        let result = self
            .conn
            .change_window_attributes(self.root, &change)?
            .check();

        match result {
            Ok(_) => {
                tracing::info!("Successfully became window manager");
                Ok(())
            }
            Err(x11rb::errors::ReplyError::X11Error(ref error))
                if error.error_kind == x11rb::protocol::ErrorKind::Access =>
            {
                Err(Error::AnotherWmRunning)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Grab a key combination on the root window.
    /// Grabs with multiple modifier combinations to handle NumLock/CapsLock.
    pub fn grab_key(&self, modifiers: ModMask, keycode: u8) -> Result<(), Error> {
        // NumLock is typically Mod2, CapsLock is Lock
        let numlock = ModMask::M2;
        let capslock = ModMask::LOCK;

        // Grab with all combinations of NumLock/CapsLock
        let variants = [
            modifiers,
            modifiers | numlock,
            modifiers | capslock,
            modifiers | numlock | capslock,
        ];

        for mods in variants {
            self.conn.grab_key(
                false,
                self.root,
                mods,
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?;
        }
        Ok(())
    }

    /// Grab mouse button for click-to-focus on a window.
    pub fn grab_button(&self, window: Window) -> Result<(), Error> {
        self.conn.grab_button(
            false,
            window,
            EventMask::BUTTON_PRESS,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            ButtonIndex::ANY,
            ModMask::ANY,
        )?;
        Ok(())
    }

    /// Ungrab mouse button from a window (when focused, to allow click-through).
    pub fn ungrab_button(&self, window: Window) -> Result<(), Error> {
        self.conn
            .ungrab_button(ButtonIndex::ANY, window, ModMask::ANY)?;
        Ok(())
    }

    /// Grab Alt+Button1 and Alt+Button3 on root for floating window move/resize.
    pub fn grab_mod_buttons(&self) -> Result<(), Error> {
        let numlock = ModMask::M2;
        let capslock = ModMask::LOCK;

        // Grab Alt+Button1 (move) and Alt+Button3 (resize) with NumLock/CapsLock variants
        for button in [ButtonIndex::M1, ButtonIndex::M3] {
            for mods in [
                ModMask::M1,
                ModMask::M1 | numlock,
                ModMask::M1 | capslock,
                ModMask::M1 | numlock | capslock,
            ] {
                self.conn.grab_button(
                    false,
                    self.root,
                    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::BUTTON_MOTION,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    x11rb::NONE,
                    x11rb::NONE,
                    button,
                    mods,
                )?;
            }
        }
        tracing::debug!("Grabbed Alt+Button1/Button3 on root for floating move/resize");
        Ok(())
    }

    /// Set input focus to a window.
    pub fn set_focus(&self, window: Window) -> Result<(), Error> {
        self.conn
            .set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;
        Ok(())
    }

    /// Set window border width and color.
    pub fn set_border(&self, window: Window, width: u32, color: u32) -> Result<(), Error> {
        let aux = ChangeWindowAttributesAux::new().border_pixel(color);
        self.conn.change_window_attributes(window, &aux)?;

        let configure = ConfigureWindowAux::new().border_width(width);
        self.conn.configure_window(window, &configure)?;
        Ok(())
    }

    /// Configure a window's geometry.
    pub fn configure_window(
        &self,
        window: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u32,
    ) -> Result<(), Error> {
        let aux = ConfigureWindowAux::new()
            .x(x as i32)
            .y(y as i32)
            .width(width as u32)
            .height(height as u32)
            .border_width(border_width);
        self.conn.configure_window(window, &aux)?;
        Ok(())
    }

    /// Map (show) a window.
    pub fn map_window(&self, window: Window) -> Result<(), Error> {
        self.conn.map_window(window)?;
        Ok(())
    }

    /// Unmap (hide) a window.
    pub fn unmap_window(&self, window: Window) -> Result<(), Error> {
        self.conn.unmap_window(window)?;
        Ok(())
    }

    /// Grab the pointer for drag operations.
    pub fn grab_pointer(&self, _window: Window) -> Result<(), Error> {
        self.conn.grab_pointer(
            false,
            self.root,
            EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            CURRENT_TIME,
        )?;
        Ok(())
    }

    /// Release pointer grab.
    pub fn ungrab_pointer(&self) -> Result<(), Error> {
        self.conn.ungrab_pointer(CURRENT_TIME)?;
        Ok(())
    }

    /// Subscribe to events on a window.
    pub fn select_input(&self, window: Window, mask: EventMask) -> Result<(), Error> {
        let aux = ChangeWindowAttributesAux::new().event_mask(mask);
        self.conn.change_window_attributes(window, &aux)?;
        Ok(())
    }

    /// Get keycode for a keysym.
    pub fn keycode_from_keysym(&self, keysym: u32) -> Option<u8> {
        let setup = self.conn.setup();
        let min_keycode = setup.min_keycode;
        let max_keycode = setup.max_keycode;

        // Get the keyboard mapping
        if let Ok(cookie) = self.conn.get_keyboard_mapping(min_keycode, max_keycode - min_keycode + 1) {
            if let Ok(reply) = cookie.reply() {
                let keysyms_per_keycode = reply.keysyms_per_keycode as usize;
                for i in 0..((max_keycode - min_keycode + 1) as usize) {
                    for j in 0..keysyms_per_keycode {
                        if reply.keysyms[i * keysyms_per_keycode + j] == keysym {
                            return Some(min_keycode + i as u8);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn flush(&self) -> Result<(), Error> {
        self.conn.flush()?;
        Ok(())
    }

    pub fn sync(&self) -> Result<(), Error> {
        self.conn.sync()?;
        Ok(())
    }

    /// Check if a window supports the WM_DELETE_WINDOW protocol.
    pub fn supports_delete_window(&self, window: Window) -> bool {
        match self.conn.get_property(
            false,
            window,
            self.wm_protocols,
            AtomEnum::ATOM,
            0,
            1024,
        ) {
            Ok(cookie) => {
                if let Ok(reply) = cookie.reply() {
                    if reply.type_ == u32::from(AtomEnum::ATOM) && reply.format == 32 {
                        // Parse atoms from the value (32-bit little-endian values)
                        for chunk in reply.value.chunks_exact(4) {
                            let atom = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                            if atom == self.wm_delete_window {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            Err(_) => false,
        }
    }

    /// Send WM_DELETE_WINDOW client message to gracefully close a window.
    pub fn send_delete_window(&self, window: Window) -> Result<(), Error> {
        let data = ClientMessageData::from([
            self.wm_delete_window,
            CURRENT_TIME,
            0,
            0,
            0,
        ]);

        let event = ClientMessageEvent::new(32, window, self.wm_protocols, data);

        self.conn.send_event(false, window, EventMask::NO_EVENT, event)?;
        Ok(())
    }

    /// Check if a window should automatically float based on ICCCM/EWMH hints.
    /// Returns true for dialogs, transients, utilities, toolbars, splashes, etc.
    pub fn should_float(&self, window: Window) -> bool {
        // 1. Check WM_TRANSIENT_FOR (ICCCM) - dialogs and popups
        if let Ok(cookie) = self.conn.get_property(
            false,
            window,
            self.wm_transient_for,
            AtomEnum::WINDOW,
            0,
            1,
        ) {
            if let Ok(reply) = cookie.reply() {
                if reply.type_ == u32::from(AtomEnum::WINDOW) && !reply.value.is_empty() {
                    tracing::debug!("Window {} has WM_TRANSIENT_FOR, should float", window);
                    return true;
                }
            }
        }

        // 2. Check _NET_WM_WINDOW_TYPE (EWMH)
        if let Ok(cookie) = self.conn.get_property(
            false,
            window,
            self.net_wm_window_type,
            AtomEnum::ATOM,
            0,
            32,
        ) {
            if let Ok(reply) = cookie.reply() {
                if reply.type_ == u32::from(AtomEnum::ATOM) && reply.format == 32 {
                    let float_types = [
                        self.net_wm_window_type_dialog,
                        self.net_wm_window_type_utility,
                        self.net_wm_window_type_toolbar,
                        self.net_wm_window_type_splash,
                        self.net_wm_window_type_notification,
                    ];

                    for chunk in reply.value.chunks_exact(4) {
                        let atom = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        if float_types.contains(&atom) {
                            tracing::debug!("Window {} has floating window type, should float", window);
                            return true;
                        }
                    }
                }
            }
        }

        // 3. Check _NET_WM_STATE for modal windows
        if let Ok(cookie) = self.conn.get_property(
            false,
            window,
            self.net_wm_state,
            AtomEnum::ATOM,
            0,
            32,
        ) {
            if let Ok(reply) = cookie.reply() {
                if reply.type_ == u32::from(AtomEnum::ATOM) && reply.format == 32 {
                    for chunk in reply.value.chunks_exact(4) {
                        let atom = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        if atom == self.net_wm_state_modal {
                            tracing::debug!("Window {} is modal, should float", window);
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Get WM_CLASS property (instance, class) for a window.
    pub fn get_wm_class(&self, window: Window) -> Option<(String, String)> {
        let wm_class_atom = self.conn.intern_atom(false, b"WM_CLASS").ok()?.reply().ok()?.atom;
        let string_atom = self.conn.intern_atom(false, b"STRING").ok()?.reply().ok()?.atom;

        let cookie = self.conn.get_property(false, window, wm_class_atom, string_atom, 0, 1024).ok()?;
        let reply = cookie.reply().ok()?;

        if reply.format != 8 {
            return None;
        }

        // WM_CLASS is two null-terminated strings: instance\0class\0
        let value = reply.value;
        let mut parts = value.split(|&b| b == 0).filter(|s| !s.is_empty());

        let instance = parts.next().and_then(|s| String::from_utf8(s.to_vec()).ok())?;
        let class = parts.next().and_then(|s| String::from_utf8(s.to_vec()).ok())?;

        Some((instance, class))
    }

    /// Get _NET_WM_NAME or WM_NAME for a window.
    pub fn get_window_title(&self, window: Window) -> Option<String> {
        // Try _NET_WM_NAME first (UTF-8)
        let net_wm_name = self.conn.intern_atom(false, b"_NET_WM_NAME").ok()?.reply().ok()?.atom;
        let utf8_string = self.conn.intern_atom(false, b"UTF8_STRING").ok()?.reply().ok()?.atom;

        if let Ok(cookie) = self.conn.get_property(false, window, net_wm_name, utf8_string, 0, 1024) {
            if let Ok(reply) = cookie.reply() {
                if reply.format == 8 && !reply.value.is_empty() {
                    return String::from_utf8(reply.value).ok();
                }
            }
        }

        // Fall back to WM_NAME
        let wm_name = self.conn.intern_atom(false, b"WM_NAME").ok()?.reply().ok()?.atom;
        let string_atom = self.conn.intern_atom(false, b"STRING").ok()?.reply().ok()?.atom;

        let cookie = self.conn.get_property(false, window, wm_name, string_atom, 0, 1024).ok()?;
        let reply = cookie.reply().ok()?;

        if reply.format == 8 {
            return String::from_utf8(reply.value).ok();
        }

        None
    }

    /// Set _NET_SUPPORTED on root window to advertise supported EWMH atoms.
    pub fn set_ewmh_supported(&self) -> Result<(), Error> {
        let supported = [
            self.net_supported,
            self.net_number_of_desktops,
            self.net_current_desktop,
            self.net_desktop_names,
            self.net_wm_desktop,
            self.net_active_window,
        ];
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_supported,
            AtomEnum::ATOM,
            &supported,
        )?;
        Ok(())
    }

    /// Set _NET_NUMBER_OF_DESKTOPS on root window.
    pub fn set_number_of_desktops(&self, count: u32) -> Result<(), Error> {
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_number_of_desktops,
            AtomEnum::CARDINAL,
            &[count],
        )?;
        Ok(())
    }

    /// Set _NET_CURRENT_DESKTOP on root window.
    pub fn set_current_desktop(&self, index: u32) -> Result<(), Error> {
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_current_desktop,
            AtomEnum::CARDINAL,
            &[index],
        )?;
        Ok(())
    }

    /// Set _NET_DESKTOP_NAMES on root window.
    pub fn set_desktop_names(&self, names: &[String]) -> Result<(), Error> {
        // Names are null-terminated UTF8 strings concatenated together
        let mut data: Vec<u8> = Vec::new();
        for name in names {
            data.extend_from_slice(name.as_bytes());
            data.push(0);
        }
        self.conn.change_property8(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_desktop_names,
            self.utf8_string,
            &data,
        )?;
        Ok(())
    }

    /// Set _NET_ACTIVE_WINDOW on root window.
    pub fn set_active_window(&self, window: Option<Window>) -> Result<(), Error> {
        let win = window.unwrap_or(0);
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_active_window,
            AtomEnum::WINDOW,
            &[win],
        )?;
        Ok(())
    }

    /// Set _NET_WM_DESKTOP on a window.
    pub fn set_window_desktop(&self, window: Window, desktop: u32) -> Result<(), Error> {
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            window,
            self.net_wm_desktop,
            AtomEnum::CARDINAL,
            &[desktop],
        )?;
        Ok(())
    }

    /// Detect connected monitors via RandR.
    pub fn detect_monitors(&self) -> Result<Vec<crate::core::Monitor>, Error> {
        use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};
        use crate::core::{Monitor, Rect};

        let resources = self.conn.randr_get_screen_resources(self.root)?.reply()?;
        let primary = self.conn.randr_get_output_primary(self.root)?.reply()?.output;

        let mut monitors = Vec::new();

        for &output in &resources.outputs {
            let info = match self.conn.randr_get_output_info(output, 0)?.reply() {
                Ok(info) => info,
                Err(_) => continue,
            };

            // Skip disconnected outputs
            if info.connection != randr::Connection::CONNECTED {
                continue;
            }

            // Skip outputs without a CRTC (not active)
            let crtc = match info.crtc {
                0 => continue,
                c => c,
            };

            let crtc_info = match self.conn.randr_get_crtc_info(crtc, 0)?.reply() {
                Ok(info) => info,
                Err(_) => continue,
            };

            let name = String::from_utf8_lossy(&info.name).to_string();
            let geometry = Rect::new(
                crtc_info.x,
                crtc_info.y,
                crtc_info.width,
                crtc_info.height,
            );

            let mut monitor = Monitor::new(name, output, geometry);
            monitor.primary = output == primary;

            monitors.push(monitor);
        }

        // Sort monitors by X position (left to right)
        monitors.sort_by_key(|m| m.geometry.x);

        tracing::info!("Detected {} monitors: {:?}",
            monitors.len(),
            monitors.iter().map(|m| &m.name).collect::<Vec<_>>()
        );

        Ok(monitors)
    }

    /// Subscribe to RandR screen change events.
    pub fn subscribe_randr_events(&self) -> Result<(), Error> {
        use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};

        self.conn.randr_select_input(
            self.root,
            randr::NotifyMask::SCREEN_CHANGE | randr::NotifyMask::OUTPUT_CHANGE,
        )?;
        Ok(())
    }
}

impl std::ops::Deref for Connection {
    type Target = RustConnection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}
