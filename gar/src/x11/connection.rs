use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ButtonIndex, ChangeWindowAttributesAux, ClientMessageData,
    ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, Font, GrabMode, InputFocus,
    ModMask, Screen, StackMode, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;
use x11rb::CURRENT_TIME;

use super::Error;

/// Parsed WM_HINTS structure (ICCCM).
#[derive(Debug, Clone, Default)]
pub struct WmHints {
    pub input: bool,
    pub initial_state: Option<u32>,
    pub urgent: bool,
}

/// Parsed WM_NORMAL_HINTS (size hints) structure (ICCCM).
#[derive(Debug, Clone, Default)]
pub struct SizeHints {
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub base_width: Option<u32>,
    pub base_height: Option<u32>,
    pub width_inc: Option<u32>,
    pub height_inc: Option<u32>,
}

/// Reserved screen area (strut) from _NET_WM_STRUT or _NET_WM_STRUT_PARTIAL.
#[derive(Debug, Clone, Copy, Default)]
pub struct Strut {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

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
    pub wm_hints: Atom,
    pub wm_normal_hints: Atom,
    // EWMH atoms for window types
    pub net_wm_window_type: Atom,
    pub net_wm_window_type_dialog: Atom,
    pub net_wm_window_type_utility: Atom,
    pub net_wm_window_type_toolbar: Atom,
    pub net_wm_window_type_splash: Atom,
    pub net_wm_window_type_notification: Atom,
    pub net_wm_window_type_dock: Atom,
    pub net_wm_window_type_desktop: Atom,
    // EWMH atoms for window state
    pub net_wm_state: Atom,
    pub net_wm_state_modal: Atom,
    pub net_wm_state_fullscreen: Atom,
    // EWMH atoms for workspaces
    pub net_supported: Atom,
    pub net_supporting_wm_check: Atom,
    pub net_client_list: Atom,
    pub net_client_list_stacking: Atom,
    pub net_close_window: Atom,
    pub net_wm_name: Atom,
    pub net_number_of_desktops: Atom,
    pub net_current_desktop: Atom,
    pub net_desktop_names: Atom,
    pub net_wm_desktop: Atom,
    pub net_active_window: Atom,
    pub utf8_string: Atom,
    // Compositor integration
    pub net_wm_bypass_compositor: Atom,
    // Struts (reserved screen areas for docks/panels)
    pub net_wm_strut: Atom,
    pub net_wm_strut_partial: Atom,
    // Cursors for resize/move operations
    pub cursor_normal: u32,
    pub cursor_move: u32,
    pub cursor_top_left: u32,
    pub cursor_top_right: u32,
    pub cursor_bottom_left: u32,
    pub cursor_bottom_right: u32,
    pub cursor_left: u32,
    pub cursor_right: u32,
    pub cursor_top: u32,
    pub cursor_bottom: u32,
    pub cursor_h_double: u32,  // sb_h_double_arrow - horizontal resize
    pub cursor_v_double: u32,  // sb_v_double_arrow - vertical resize
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
        let wm_hints = conn.intern_atom(false, b"WM_HINTS")?.reply()?.atom;
        let wm_normal_hints = conn.intern_atom(false, b"WM_NORMAL_HINTS")?.reply()?.atom;

        // Intern EWMH atoms for window types
        let net_wm_window_type = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?.reply()?.atom;
        let net_wm_window_type_dialog = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DIALOG")?.reply()?.atom;
        let net_wm_window_type_utility = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY")?.reply()?.atom;
        let net_wm_window_type_toolbar = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_TOOLBAR")?.reply()?.atom;
        let net_wm_window_type_splash = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_SPLASH")?.reply()?.atom;
        let net_wm_window_type_notification = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_NOTIFICATION")?.reply()?.atom;
        let net_wm_window_type_dock = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DOCK")?.reply()?.atom;
        let net_wm_window_type_desktop = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DESKTOP")?.reply()?.atom;

        // Intern EWMH atoms for window state
        let net_wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
        let net_wm_state_modal = conn.intern_atom(false, b"_NET_WM_STATE_MODAL")?.reply()?.atom;
        let net_wm_state_fullscreen = conn.intern_atom(false, b"_NET_WM_STATE_FULLSCREEN")?.reply()?.atom;

        // Intern EWMH atoms for workspaces and WM identification
        let net_supported = conn.intern_atom(false, b"_NET_SUPPORTED")?.reply()?.atom;
        let net_supporting_wm_check = conn.intern_atom(false, b"_NET_SUPPORTING_WM_CHECK")?.reply()?.atom;
        let net_client_list = conn.intern_atom(false, b"_NET_CLIENT_LIST")?.reply()?.atom;
        let net_client_list_stacking = conn.intern_atom(false, b"_NET_CLIENT_LIST_STACKING")?.reply()?.atom;
        let net_close_window = conn.intern_atom(false, b"_NET_CLOSE_WINDOW")?.reply()?.atom;
        let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
        let net_number_of_desktops = conn.intern_atom(false, b"_NET_NUMBER_OF_DESKTOPS")?.reply()?.atom;
        let net_current_desktop = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?.reply()?.atom;
        let net_desktop_names = conn.intern_atom(false, b"_NET_DESKTOP_NAMES")?.reply()?.atom;
        let net_wm_desktop = conn.intern_atom(false, b"_NET_WM_DESKTOP")?.reply()?.atom;
        let net_active_window = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
        let utf8_string = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

        // Compositor integration atom - apps can set this to request un-redirection for fullscreen
        let net_wm_bypass_compositor = conn.intern_atom(false, b"_NET_WM_BYPASS_COMPOSITOR")?.reply()?.atom;

        // Strut atoms for dock/panel reserved areas
        let net_wm_strut = conn.intern_atom(false, b"_NET_WM_STRUT")?.reply()?.atom;
        let net_wm_strut_partial = conn.intern_atom(false, b"_NET_WM_STRUT_PARTIAL")?.reply()?.atom;

        // Create cursors for pointer and resize operations
        let (
            cursor_normal,
            cursor_move,
            cursor_top_left,
            cursor_top_right,
            cursor_bottom_left,
            cursor_bottom_right,
            cursor_left,
            cursor_right,
            cursor_top,
            cursor_bottom,
            cursor_h_double,
            cursor_v_double,
        ) = Self::create_cursors(&conn)?;

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
            wm_hints,
            wm_normal_hints,
            net_wm_window_type,
            net_wm_window_type_dialog,
            net_wm_window_type_utility,
            net_wm_window_type_toolbar,
            net_wm_window_type_splash,
            net_wm_window_type_notification,
            net_wm_window_type_dock,
            net_wm_window_type_desktop,
            net_wm_state,
            net_wm_state_modal,
            net_wm_state_fullscreen,
            net_supported,
            net_supporting_wm_check,
            net_client_list,
            net_client_list_stacking,
            net_close_window,
            net_wm_name,
            net_number_of_desktops,
            net_current_desktop,
            net_desktop_names,
            net_wm_desktop,
            net_active_window,
            utf8_string,
            net_wm_bypass_compositor,
            net_wm_strut,
            net_wm_strut_partial,
            cursor_normal,
            cursor_move,
            cursor_top_left,
            cursor_top_right,
            cursor_bottom_left,
            cursor_bottom_right,
            cursor_left,
            cursor_right,
            cursor_top,
            cursor_bottom,
            cursor_h_double,
            cursor_v_double,
        })
    }

    pub fn screen(&self) -> &Screen {
        &self.conn.setup().roots[self.screen_num]
    }

    pub fn become_wm(&self) -> Result<(), Error> {
        // Set root window background to black, cursor, and subscribe to events
        // The background ensures old window pixels are cleared when windows close
        let change = ChangeWindowAttributesAux::new()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::PROPERTY_CHANGE,
            )
            .background_pixel(self.screen().black_pixel)
            .cursor(self.cursor_normal);

        let result = self
            .conn
            .change_window_attributes(self.root, &change)?
            .check();

        // Clear the root window to apply the background
        if result.is_ok() {
            self.conn.clear_area(
                false,
                self.root,
                0,
                0,
                self.screen_width,
                self.screen_height,
            )?;

            // Force cursor refresh by warping pointer in place
            // This clears any stale cursor artifacts from display manager
            if let Ok(reply) = self.conn.query_pointer(self.root)?.reply() {
                self.conn.warp_pointer(
                    x11rb::NONE,
                    self.root,
                    0, 0, 0, 0,
                    reply.root_x,
                    reply.root_y,
                )?;
            }

            self.conn.flush()?;
        }

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

    /// Create all cursors used by the window manager.
    fn create_cursors(conn: &RustConnection) -> Result<(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32), Error> {
        // Open the cursor font
        let font: Font = conn.generate_id()?;
        conn.open_font(font, b"cursor")?;

        // Cursor glyph numbers from the cursor font:
        // left_ptr = 68, fleur = 52, top_left_corner = 134, top_right_corner = 136
        // bottom_left_corner = 12, bottom_right_corner = 14
        // left_side = 70, right_side = 96, top_side = 138, bottom_side = 16

        let create = |glyph: u16| -> Result<u32, Error> {
            let cursor = conn.generate_id()?;
            conn.create_glyph_cursor(
                cursor,
                font,
                font,
                glyph,
                glyph + 1,
                0, 0, 0,
                0xFFFF, 0xFFFF, 0xFFFF,
            )?;
            Ok(cursor)
        };

        let cursor_normal = create(68)?;       // left_ptr
        let cursor_move = create(52)?;         // fleur (move cursor)
        let cursor_top_left = create(134)?;    // top_left_corner
        let cursor_top_right = create(136)?;   // top_right_corner
        let cursor_bottom_left = create(12)?;  // bottom_left_corner
        let cursor_bottom_right = create(14)?; // bottom_right_corner
        let cursor_left = create(70)?;         // left_side
        let cursor_right = create(96)?;        // right_side
        let cursor_top = create(138)?;         // top_side
        let cursor_bottom = create(16)?;       // bottom_side
        let cursor_h_double = create(108)?;    // sb_h_double_arrow
        let cursor_v_double = create(116)?;    // sb_v_double_arrow

        // Close font (cursors keep their own references)
        conn.close_font(font)?;

        Ok((
            cursor_normal,
            cursor_move,
            cursor_top_left,
            cursor_top_right,
            cursor_bottom_left,
            cursor_bottom_right,
            cursor_left,
            cursor_right,
            cursor_top,
            cursor_bottom,
            cursor_h_double,
            cursor_v_double,
        ))
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

    /// Grab Mod+Button1 and Mod+Button3 on root for floating window move/resize.
    /// Grabs both Alt (M1) and Super (M4) to support either mod key configuration.
    pub fn grab_mod_buttons(&self) -> Result<(), Error> {
        let numlock = ModMask::M2;
        let capslock = ModMask::LOCK;

        // Grab both Alt (M1) and Super (M4) with Button1 (move) and Button3 (resize)
        for button in [ButtonIndex::M1, ButtonIndex::M3] {
            // Alt variants
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
            // Super variants
            for mods in [
                ModMask::M4,
                ModMask::M4 | numlock,
                ModMask::M4 | capslock,
                ModMask::M4 | numlock | capslock,
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
        tracing::debug!("Grabbed Mod+Button1/Button3 on root for floating move/resize");
        Ok(())
    }

    /// Grab Button1 on root without modifiers to catch clicks in gaps between tiled windows.
    pub fn grab_button1_on_root(&self) -> Result<(), Error> {
        let numlock = ModMask::M2;
        let capslock = ModMask::LOCK;

        // Grab Button1 without mod key (but handle numlock/capslock variants)
        for mods in [
            ModMask::from(0u16),
            numlock,
            capslock,
            numlock | capslock,
        ] {
            self.conn.grab_button(
                false,
                self.root,
                EventMask::BUTTON_PRESS,
                GrabMode::SYNC, // Sync mode so we can replay to client if needed
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                ButtonIndex::M1,
                mods,
            )?;
        }
        tracing::debug!("Grabbed Button1 on root for gap edge resize");
        Ok(())
    }

    /// Set the cursor for a window.
    pub fn set_window_cursor(&self, window: Window, cursor: u32) -> Result<(), Error> {
        tracing::debug!("set_window_cursor: window={} cursor={}", window, cursor);
        let change = ChangeWindowAttributesAux::new().cursor(cursor);
        self.conn.change_window_attributes(window, &change)?;
        Ok(())
    }

    /// Clear the cursor attribute from a window, letting the application's cursor show.
    pub fn clear_window_cursor(&self, window: Window) -> Result<(), Error> {
        tracing::debug!("clear_window_cursor: window={}", window);
        let change = ChangeWindowAttributesAux::new().cursor(x11rb::NONE);
        self.conn.change_window_attributes(window, &change)?;
        Ok(())
    }

    /// Set the cursor on the root window.
    pub fn set_root_cursor(&self, cursor: u32) -> Result<(), Error> {
        tracing::debug!("set_root_cursor: cursor={}", cursor);
        let change = ChangeWindowAttributesAux::new().cursor(cursor);
        self.conn.change_window_attributes(self.root, &change)?;
        Ok(())
    }

    /// Set input focus to a window.
    pub fn set_focus(&self, window: Window) -> Result<(), Error> {
        self.conn
            .set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;
        Ok(())
    }

    /// Warp the mouse pointer to the center of a window.
    pub fn warp_pointer_to_window(&self, window: Window) -> Result<(), Error> {
        // Get window geometry
        let geom = self.conn.get_geometry(window)?.reply()?;
        let center_x = (geom.width / 2) as i16;
        let center_y = (geom.height / 2) as i16;

        self.conn.warp_pointer(
            x11rb::NONE,  // src_window (none = don't check source)
            window,       // dst_window
            0, 0,         // src_x, src_y (ignored when src_window is none)
            0, 0,         // src_width, src_height (ignored)
            center_x,     // dst_x (relative to dst_window)
            center_y,     // dst_y (relative to dst_window)
        )?;
        Ok(())
    }

    /// Warp the mouse pointer to absolute screen coordinates.
    pub fn warp_pointer(&self, x: i16, y: i16) -> Result<(), Error> {
        self.conn.warp_pointer(
            x11rb::NONE,  // src_window
            self.root,    // dst_window (root for absolute coords)
            0, 0,         // src_x, src_y
            0, 0,         // src_width, src_height
            x,            // dst_x
            y,            // dst_y
        )?;
        Ok(())
    }

    /// Get the current mouse pointer position (root window coordinates).
    pub fn get_pointer_position(&self) -> Result<(i16, i16), Error> {
        let reply = self.conn.query_pointer(self.root)?.reply()?;
        Ok((reply.root_x, reply.root_y))
    }

    /// Set window border width and color.
    pub fn set_border(&self, window: Window, width: u32, color: u32) -> Result<(), Error> {
        let aux = ChangeWindowAttributesAux::new().border_pixel(color);
        self.conn.change_window_attributes(window, &aux)?;

        let configure = ConfigureWindowAux::new().border_width(width);
        self.conn.configure_window(window, &configure)?;
        Ok(())
    }

    /// Clear an area of the root window (fills with background color).
    pub fn clear_root_area(&self, x: i16, y: i16, width: u16, height: u16) -> Result<(), Error> {
        self.conn.clear_area(false, self.root, x, y, width, height)?;
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

    /// Raise a window to the top of the stacking order.
    pub fn raise_window(&self, window: Window) -> Result<(), Error> {
        let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        self.conn.configure_window(window, &aux)?;
        Ok(())
    }

    /// Move a window to a new position without changing size.
    pub fn move_window(&self, window: Window, x: i16, y: i16) -> Result<(), Error> {
        let aux = ConfigureWindowAux::new().x(x as i32).y(y as i32);
        self.conn.configure_window(window, &aux)?;
        Ok(())
    }

    /// Grab the pointer for drag operations with optional cursor override.
    pub fn grab_pointer(&self, cursor: Option<u32>) -> Result<(), Error> {
        let cursor_id = cursor.unwrap_or(x11rb::NONE);
        let reply = self.conn.grab_pointer(
            false,
            self.root,
            EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            cursor_id,
            CURRENT_TIME,
        )?.reply()?;
        tracing::debug!("grab_pointer result: {:?}", reply.status);
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

    /// Check if a window should be ignored entirely (not managed by the WM).
    /// Returns true for dock windows (status bars like polybar) and desktop windows.
    pub fn should_ignore(&self, window: Window) -> bool {
        // Check _NET_WM_WINDOW_TYPE for dock/desktop types
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
                    let ignore_types = [
                        self.net_wm_window_type_dock,
                        self.net_wm_window_type_desktop,
                    ];

                    for chunk in reply.value.chunks_exact(4) {
                        let atom = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        if ignore_types.contains(&atom) {
                            tracing::debug!("Window {} is dock/desktop type, ignoring", window);
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

    /// Get WM_HINTS for a window (ICCCM).
    /// Returns urgency flag and other hints.
    pub fn get_wm_hints(&self, window: Window) -> Option<WmHints> {
        let reply = self.conn.get_property(
            false,
            window,
            self.wm_hints,
            AtomEnum::ANY,
            0,
            9, // WM_HINTS has 9 32-bit values
        ).ok()?.reply().ok()?;

        if reply.format != 32 || reply.value.len() < 4 {
            return None;
        }

        // Parse the 32-bit values
        let values: Vec<u32> = reply.value
            .chunks_exact(4)
            .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        if values.is_empty() {
            return None;
        }

        let flags = values[0];

        // Flag bits from ICCCM
        const INPUT_HINT: u32 = 1 << 0;
        const STATE_HINT: u32 = 1 << 1;
        const URGENCY_HINT: u32 = 1 << 8; // XUrgencyHint

        let mut hints = WmHints::default();

        // Input hint (does window want keyboard focus?)
        if flags & INPUT_HINT != 0 && values.len() > 1 {
            hints.input = values[1] != 0;
        } else {
            hints.input = true; // Default to accepting input
        }

        // Initial state hint
        if flags & STATE_HINT != 0 && values.len() > 2 {
            hints.initial_state = Some(values[2]);
        }

        // Urgency hint
        hints.urgent = flags & URGENCY_HINT != 0;

        Some(hints)
    }

    /// Get WM_NORMAL_HINTS (size hints) for a window (ICCCM).
    pub fn get_size_hints(&self, window: Window) -> Option<SizeHints> {
        let reply = self.conn.get_property(
            false,
            window,
            self.wm_normal_hints,
            AtomEnum::ANY,
            0,
            18, // WM_SIZE_HINTS has up to 18 32-bit values
        ).ok()?.reply().ok()?;

        if reply.format != 32 || reply.value.len() < 4 {
            return None;
        }

        // Parse the 32-bit values
        let values: Vec<u32> = reply.value
            .chunks_exact(4)
            .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        if values.is_empty() {
            return None;
        }

        let flags = values[0];

        // Flag bits from ICCCM (indices into the values array)
        const P_MIN_SIZE: u32 = 1 << 4;    // min_width, min_height at [5], [6]
        const P_MAX_SIZE: u32 = 1 << 5;    // max_width, max_height at [7], [8]
        const P_RESIZE_INC: u32 = 1 << 6;  // width_inc, height_inc at [9], [10]
        const P_BASE_SIZE: u32 = 1 << 8;   // base_width, base_height at [15], [16]

        let mut hints = SizeHints::default();

        // Min size (indices 5, 6)
        if flags & P_MIN_SIZE != 0 && values.len() > 6 {
            hints.min_width = Some(values[5]);
            hints.min_height = Some(values[6]);
        }

        // Max size (indices 7, 8)
        if flags & P_MAX_SIZE != 0 && values.len() > 8 {
            hints.max_width = Some(values[7]);
            hints.max_height = Some(values[8]);
        }

        // Resize increment (indices 9, 10)
        if flags & P_RESIZE_INC != 0 && values.len() > 10 {
            hints.width_inc = Some(values[9]);
            hints.height_inc = Some(values[10]);
        }

        // Base size (indices 15, 16)
        if flags & P_BASE_SIZE != 0 && values.len() > 16 {
            hints.base_width = Some(values[15]);
            hints.base_height = Some(values[16]);
        }

        Some(hints)
    }

    /// Get _NET_WM_STRUT or _NET_WM_STRUT_PARTIAL for a window.
    /// Returns (left, right, top, bottom) reserved pixels.
    pub fn get_strut(&self, window: Window) -> Option<Strut> {
        // Try _NET_WM_STRUT_PARTIAL first (more detailed)
        let reply = self.conn.get_property(
            false,
            window,
            self.net_wm_strut_partial,
            AtomEnum::CARDINAL,
            0,
            12, // STRUT_PARTIAL has 12 cardinals
        ).ok()?.reply().ok();

        if let Some(reply) = reply {
            if reply.format == 32 && reply.value.len() >= 16 {
                let values: Vec<u32> = reply.value
                    .chunks_exact(4)
                    .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();

                if values.len() >= 4 {
                    return Some(Strut {
                        left: values[0],
                        right: values[1],
                        top: values[2],
                        bottom: values[3],
                    });
                }
            }
        }

        // Fall back to _NET_WM_STRUT (simpler, 4 cardinals)
        let reply = self.conn.get_property(
            false,
            window,
            self.net_wm_strut,
            AtomEnum::CARDINAL,
            0,
            4,
        ).ok()?.reply().ok()?;

        if reply.format == 32 && reply.value.len() >= 16 {
            let values: Vec<u32> = reply.value
                .chunks_exact(4)
                .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();

            if values.len() >= 4 {
                return Some(Strut {
                    left: values[0],
                    right: values[1],
                    top: values[2],
                    bottom: values[3],
                });
            }
        }

        None
    }

    /// Set _NET_SUPPORTED on root window to advertise supported EWMH atoms.
    pub fn set_ewmh_supported(&self) -> Result<(), Error> {
        let supported = [
            self.net_supported,
            self.net_supporting_wm_check,
            self.net_client_list,
            self.net_client_list_stacking,
            self.net_number_of_desktops,
            self.net_current_desktop,
            self.net_desktop_names,
            self.net_wm_desktop,
            self.net_active_window,
            self.net_close_window,
            self.net_wm_state,
            self.net_wm_state_fullscreen,
            self.net_wm_name,
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

    /// Setup _NET_SUPPORTING_WM_CHECK window and set _NET_WM_NAME.
    /// Returns the check window ID for cleanup.
    pub fn setup_wm_check(&self) -> Result<Window, Error> {
        use x11rb::protocol::xproto::{CreateWindowAux, WindowClass, PropMode};

        // Create a small off-screen window for WM identification
        let check_window = self.conn.generate_id()?;
        self.conn.create_window(
            0, // depth: copy from parent
            check_window,
            self.root,
            -1, -1, 1, 1, // x, y, width, height (off-screen)
            0, // border_width
            WindowClass::INPUT_OUTPUT,
            0, // visual: copy from parent
            &CreateWindowAux::new(),
        )?;

        // Set _NET_SUPPORTING_WM_CHECK on root window pointing to check window
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root,
            self.net_supporting_wm_check,
            AtomEnum::WINDOW,
            &[check_window],
        )?;

        // Set _NET_SUPPORTING_WM_CHECK on check window pointing to itself
        self.conn.change_property32(
            PropMode::REPLACE,
            check_window,
            self.net_supporting_wm_check,
            AtomEnum::WINDOW,
            &[check_window],
        )?;

        // Set _NET_WM_NAME on check window to "gar"
        self.conn.change_property8(
            PropMode::REPLACE,
            check_window,
            self.net_wm_name,
            self.utf8_string,
            b"gar",
        )?;

        tracing::info!("Created WM check window {}", check_window);
        Ok(check_window)
    }

    /// Update _NET_CLIENT_LIST on root window with all managed windows.
    pub fn update_client_list(&self, windows: &[Window]) -> Result<(), Error> {
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_client_list,
            AtomEnum::WINDOW,
            windows,
        )?;
        Ok(())
    }

    /// Update _NET_CLIENT_LIST_STACKING on root window with windows in stacking order.
    pub fn update_client_list_stacking(&self, windows: &[Window]) -> Result<(), Error> {
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            self.root,
            self.net_client_list_stacking,
            AtomEnum::WINDOW,
            windows,
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

    /// Set _NET_WM_STATE on a window with the specified state atoms.
    pub fn set_window_state(&self, window: Window, states: &[Atom]) -> Result<(), Error> {
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            window,
            self.net_wm_state,
            AtomEnum::ATOM,
            states,
        )?;
        Ok(())
    }

    /// Set _NET_WM_BYPASS_COMPOSITOR on a window.
    /// Value: 0 = no preference, 1 = bypass compositor, 2 = don't bypass
    /// Setting to 1 tells picom to not apply blur/shadows/rounded corners to this window.
    pub fn set_bypass_compositor(&self, window: Window, bypass: bool) -> Result<(), Error> {
        let value: u32 = if bypass { 1 } else { 0 };
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            window,
            self.net_wm_bypass_compositor,
            AtomEnum::CARDINAL,
            &[value],
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

        // Log raw (pre-sorted) monitor info from RandR
        tracing::debug!("Raw monitor order from RandR:");
        for m in &monitors {
            tracing::debug!(
                "  '{}' at ({}, {}) size {}x{}",
                m.name, m.geometry.x, m.geometry.y, m.geometry.width, m.geometry.height
            );
        }

        // Sort monitors by X position (left to right)
        monitors.sort_by_key(|m| m.geometry.x);

        // Log sorted monitor info
        for (i, m) in monitors.iter().enumerate() {
            tracing::info!(
                "Monitor {}: '{}' at ({}, {}) size {}x{} {}",
                i, m.name, m.geometry.x, m.geometry.y,
                m.geometry.width, m.geometry.height,
                if m.primary { "(primary)" } else { "" }
            );
        }

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
