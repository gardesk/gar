use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    ButtonIndex, ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt, EventMask,
    GrabMode, InputFocus, ModMask, Screen, Window,
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
    pub fn grab_key(&self, modifiers: ModMask, keycode: u8) -> Result<(), Error> {
        self.conn.grab_key(
            false,
            self.root,
            modifiers,
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )?;
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
}

impl std::ops::Deref for Connection {
    type Target = RustConnection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}
