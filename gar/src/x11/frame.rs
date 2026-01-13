//! Frame window management for title bars and gradient borders.
//!
//! When title bars or gradient borders are enabled, client windows are reparented into frame windows.
//! The frame handles the title bar drawing, gradient border rendering, and the client sits below/inside it.

use std::collections::HashMap;

use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt, CreateGCAux, CreateWindowAux,
    EventMask, Gcontext, Rectangle, Window, WindowClass,
};
use x11rb::COPY_DEPTH_FROM_PARENT;

use super::Error;

/// Manages frame windows for title bars.
pub struct FrameManager {
    /// Map from client window to frame window
    client_to_frame: HashMap<Window, Window>,
    /// Map from frame window to client window
    frame_to_client: HashMap<Window, Window>,
    /// Graphics context for drawing (reused)
    gc: Option<Gcontext>,
}

impl FrameManager {
    pub fn new() -> Self {
        Self {
            client_to_frame: HashMap::new(),
            frame_to_client: HashMap::new(),
            gc: None,
        }
    }

    /// Check if a window is a frame we created.
    pub fn is_frame(&self, window: Window) -> bool {
        self.frame_to_client.contains_key(&window)
    }

    /// Get the client window for a frame.
    pub fn client_for_frame(&self, frame: Window) -> Option<Window> {
        self.frame_to_client.get(&frame).copied()
    }

    /// Get the frame window for a client.
    pub fn frame_for_client(&self, client: Window) -> Option<Window> {
        self.client_to_frame.get(&client).copied()
    }

    /// Create a frame window for a client and reparent the client into it.
    pub fn create_frame<C: X11Connection>(
        &mut self,
        conn: &C,
        root: Window,
        client: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        titlebar_height: u16,
        border_width: u16,
        border_color: u32,
        bg_color: u32,
    ) -> Result<Window, Error> {
        // Total frame height includes title bar + client area
        let frame_height = height + titlebar_height;

        // Generate a new window ID for the frame
        let frame = conn.generate_id()?;

        // Create the frame window
        let aux = CreateWindowAux::new()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::ENTER_WINDOW
                    | EventMask::EXPOSURE,
            )
            .background_pixel(bg_color)
            .border_pixel(border_color);

        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame,
            root,
            x,
            y,
            width,
            frame_height,
            border_width,
            WindowClass::INPUT_OUTPUT,
            0, // CopyFromParent visual
            &aux,
        )?;

        // Reparent the client window into the frame, below the title bar
        conn.reparent_window(client, frame, 0, titlebar_height as i16)?;

        // Configure the client to fill the frame width
        let client_aux = ConfigureWindowAux::new()
            .x(0)
            .y(titlebar_height as i32)
            .width(width as u32)
            .height(height as u32)
            .border_width(0); // No border on client inside frame
        conn.configure_window(client, &client_aux)?;

        // Track the mapping
        self.client_to_frame.insert(client, frame);
        self.frame_to_client.insert(frame, client);

        tracing::debug!(
            "Created frame {} for client {} at ({}, {}) size {}x{} (titlebar: {})",
            frame, client, x, y, width, frame_height, titlebar_height
        );

        Ok(frame)
    }

    /// Destroy a frame and reparent the client back to root.
    pub fn destroy_frame<C: X11Connection>(
        &mut self,
        conn: &C,
        root: Window,
        client: Window,
    ) -> Result<(), Error> {
        if let Some(frame) = self.client_to_frame.remove(&client) {
            self.frame_to_client.remove(&frame);

            // Unmap frame first to prevent visual flash of empty frame background
            let _ = conn.unmap_window(frame);

            // Reparent client back to root (may fail if client was already destroyed)
            // Ignore errors since the client might already be gone
            let _ = conn.reparent_window(client, root, 0, 0);

            // Destroy the frame window
            let _ = conn.destroy_window(frame);

            tracing::debug!("Destroyed frame {} for client {}", frame, client);
        }
        Ok(())
    }

    /// Configure a frame's geometry.
    pub fn configure_frame<C: X11Connection>(
        &self,
        conn: &C,
        client: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        titlebar_height: u16,
        border_width: u16,
    ) -> Result<(), Error> {
        if let Some(&frame) = self.client_to_frame.get(&client) {
            let frame_height = height + titlebar_height;

            // Configure frame position and size
            let frame_aux = ConfigureWindowAux::new()
                .x(x as i32)
                .y(y as i32)
                .width(width as u32)
                .height(frame_height as u32)
                .border_width(border_width as u32);
            conn.configure_window(frame, &frame_aux)?;

            // Configure client within frame
            let client_aux = ConfigureWindowAux::new()
                .x(0)
                .y(titlebar_height as i32)
                .width(width as u32)
                .height(height as u32);
            conn.configure_window(client, &client_aux)?;
        }
        Ok(())
    }

    /// Draw a title bar on the frame.
    pub fn draw_titlebar<C: X11Connection>(
        &mut self,
        conn: &C,
        client: Window,
        title: &str,
        width: u16,
        titlebar_height: u16,
        bg_color: u32,
        text_color: u32,
        focused: bool,
    ) -> Result<(), Error> {
        let Some(&frame) = self.client_to_frame.get(&client) else {
            return Ok(());
        };

        // Ensure we have a GC
        let gc = match self.gc {
            Some(gc) => gc,
            None => {
                let gc = conn.generate_id()?;
                let aux = CreateGCAux::new()
                    .foreground(text_color)
                    .background(bg_color);
                conn.create_gc(gc, frame, &aux)?;
                self.gc = Some(gc);
                gc
            }
        };

        // Update GC colors
        let gc_aux = ChangeWindowAttributesAux::new();
        let _ = gc_aux; // We'll use ChangeGC instead

        // Clear the title bar area with background color
        let rect = Rectangle {
            x: 0,
            y: 0,
            width,
            height: titlebar_height,
        };

        // Set fill color and fill rectangle
        conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(bg_color))?;
        conn.poly_fill_rectangle(frame, gc, &[rect])?;

        // Draw the title text (simple, centered vertically)
        if !title.is_empty() {
            conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(text_color))?;

            // Draw text at (8, titlebar_height - 4) - simple positioning
            // X11 core fonts are limited, but this gives basic text rendering
            let text_y = titlebar_height.saturating_sub(4) as i16;
            conn.image_text8(frame, gc, 8, text_y, title.as_bytes())?;
        }

        // Add a focus indicator - subtle line at bottom
        if focused {
            let indicator_rect = Rectangle {
                x: 0,
                y: (titlebar_height - 2) as i16,
                width,
                height: 2,
            };
            conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(0x5294e2))?; // Blue accent
            conn.poly_fill_rectangle(frame, gc, &[indicator_rect])?;
        }

        Ok(())
    }

    /// Update frame border color.
    pub fn set_frame_border<C: X11Connection>(
        &self,
        conn: &C,
        client: Window,
        border_color: u32,
    ) -> Result<(), Error> {
        if let Some(&frame) = self.client_to_frame.get(&client) {
            let aux = ChangeWindowAttributesAux::new().border_pixel(border_color);
            conn.change_window_attributes(frame, &aux)?;
        }
        Ok(())
    }

    /// Map the frame window.
    pub fn map_frame<C: X11Connection>(&self, conn: &C, client: Window) -> Result<(), Error> {
        if let Some(&frame) = self.client_to_frame.get(&client) {
            conn.map_window(frame)?;
        }
        Ok(())
    }

    /// Unmap the frame window.
    pub fn unmap_frame<C: X11Connection>(&self, conn: &C, client: Window) -> Result<(), Error> {
        if let Some(&frame) = self.client_to_frame.get(&client) {
            conn.unmap_window(frame)?;
        }
        Ok(())
    }

    /// Get all frame windows.
    pub fn all_frames(&self) -> impl Iterator<Item = Window> + '_ {
        self.frame_to_client.keys().copied()
    }

    /// Draw a gradient border around the frame.
    /// This draws the border area of the frame with a color gradient.
    /// The `direction` can be "vertical", "horizontal", or "diagonal".
    pub fn draw_gradient_border<C: X11Connection>(
        &mut self,
        conn: &C,
        client: Window,
        width: u16,
        height: u16,
        border_width: u16,
        start_color: u32,
        end_color: u32,
        direction: &str,
    ) -> Result<(), Error> {
        let Some(&frame) = self.client_to_frame.get(&client) else {
            return Ok(());
        };

        if border_width == 0 {
            return Ok(());
        }

        // Ensure we have a GC
        let gc = match self.gc {
            Some(gc) => gc,
            None => {
                let gc = conn.generate_id()?;
                let aux = CreateGCAux::new().foreground(start_color);
                conn.create_gc(gc, frame, &aux)?;
                self.gc = Some(gc);
                gc
            }
        };

        // Number of gradient steps for smooth appearance
        let steps: u16 = 32.min(border_width * 2);

        match direction {
            "vertical" => {
                // Draw gradient from top to bottom across the border areas
                // We draw along the sides
                let step_height = height / steps;
                for i in 0..steps {
                    let t = i as f32 / (steps - 1).max(1) as f32;
                    let color = interpolate_color(start_color, end_color, t);
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;

                    let y = (i * step_height) as i16;
                    let seg_height = step_height.min(height.saturating_sub(i * step_height));

                    // Left border strip
                    let left_rect = Rectangle {
                        x: 0,
                        y,
                        width: border_width,
                        height: seg_height,
                    };
                    // Right border strip
                    let right_rect = Rectangle {
                        x: (width - border_width) as i16,
                        y,
                        width: border_width,
                        height: seg_height,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[left_rect, right_rect])?;
                }

                // Top and bottom borders (full width gradient)
                let step_width = width / steps;
                for i in 0..steps {
                    let t = i as f32 / (steps - 1).max(1) as f32;
                    let color = interpolate_color(start_color, end_color, t);
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;

                    let x = (i * step_width) as i16;
                    let seg_width = step_width.min(width.saturating_sub(i * step_width));

                    // Top border strip
                    let top_rect = Rectangle {
                        x,
                        y: 0,
                        width: seg_width,
                        height: border_width,
                    };
                    // Bottom border strip
                    let bottom_rect = Rectangle {
                        x,
                        y: (height - border_width) as i16,
                        width: seg_width,
                        height: border_width,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[top_rect, bottom_rect])?;
                }
            }
            "horizontal" => {
                // Draw gradient from left to right across all border areas
                let step_width = width / steps;
                for i in 0..steps {
                    let t = i as f32 / (steps - 1).max(1) as f32;
                    let color = interpolate_color(start_color, end_color, t);
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;

                    let x = (i * step_width) as i16;
                    let seg_width = step_width.min(width.saturating_sub(i * step_width));

                    // Top border
                    let top_rect = Rectangle {
                        x,
                        y: 0,
                        width: seg_width,
                        height: border_width,
                    };
                    // Bottom border
                    let bottom_rect = Rectangle {
                        x,
                        y: (height - border_width) as i16,
                        width: seg_width,
                        height: border_width,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[top_rect, bottom_rect])?;
                }

                // Side borders
                let step_height = height / steps;
                for i in 0..steps {
                    let t = i as f32 / (steps - 1).max(1) as f32;
                    let color = interpolate_color(start_color, end_color, t);
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;

                    let y = (i * step_height) as i16;
                    let seg_height = step_height.min(height.saturating_sub(i * step_height));

                    // Left border
                    let left_rect = Rectangle {
                        x: 0,
                        y,
                        width: border_width,
                        height: seg_height,
                    };
                    // Right border
                    let right_rect = Rectangle {
                        x: (width - border_width) as i16,
                        y,
                        width: border_width,
                        height: seg_height,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[left_rect, right_rect])?;
                }
            }
            "diagonal" | _ => {
                // Draw a diagonal gradient (top-left to bottom-right)
                // We approximate by using the sum of x+y position
                let max_dist = (width + height) as f32;

                // Draw borders with diagonal gradient
                // Top border
                for x in 0..width {
                    let t = x as f32 / max_dist;
                    let color = interpolate_color(start_color, end_color, t);
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;
                    let rect = Rectangle {
                        x: x as i16,
                        y: 0,
                        width: 1,
                        height: border_width,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[rect])?;
                }
                // Bottom border
                for x in 0..width {
                    let t = (x as f32 + (height - border_width) as f32) / max_dist;
                    let color = interpolate_color(start_color, end_color, t.min(1.0));
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;
                    let rect = Rectangle {
                        x: x as i16,
                        y: (height - border_width) as i16,
                        width: 1,
                        height: border_width,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[rect])?;
                }
                // Left border
                for y in border_width..(height - border_width) {
                    let t = y as f32 / max_dist;
                    let color = interpolate_color(start_color, end_color, t);
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;
                    let rect = Rectangle {
                        x: 0,
                        y: y as i16,
                        width: border_width,
                        height: 1,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[rect])?;
                }
                // Right border
                for y in border_width..(height - border_width) {
                    let t = ((width - border_width) as f32 + y as f32) / max_dist;
                    let color = interpolate_color(start_color, end_color, t.min(1.0));
                    conn.change_gc(gc, &x11rb::protocol::xproto::ChangeGCAux::new().foreground(color))?;
                    let rect = Rectangle {
                        x: (width - border_width) as i16,
                        y: y as i16,
                        width: border_width,
                        height: 1,
                    };
                    conn.poly_fill_rectangle(frame, gc, &[rect])?;
                }
            }
        }

        Ok(())
    }

    /// Create a border frame (frame without titlebar, just for gradient borders).
    /// The frame acts as a border container with the client centered inside.
    pub fn create_border_frame<C: X11Connection>(
        &mut self,
        conn: &C,
        root: Window,
        client: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        bg_color: u32,
    ) -> Result<Window, Error> {
        // Frame size includes the border on all sides
        let frame_width = width + border_width * 2;
        let frame_height = height + border_width * 2;

        // Generate a new window ID for the frame
        let frame = conn.generate_id()?;

        // Create the frame window with no X11 border (we draw our own)
        let aux = CreateWindowAux::new()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::ENTER_WINDOW
                    | EventMask::EXPOSURE,
            )
            .background_pixel(bg_color)
            .border_pixel(0);

        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame,
            root,
            x,
            y,
            frame_width,
            frame_height,
            0, // No X11 border - we draw gradients ourselves
            WindowClass::INPUT_OUTPUT,
            0, // CopyFromParent visual
            &aux,
        )?;

        // Reparent the client window into the frame, inset by border_width
        conn.reparent_window(client, frame, border_width as i16, border_width as i16)?;

        // Configure the client to fill the center of the frame
        let client_aux = ConfigureWindowAux::new()
            .x(border_width as i32)
            .y(border_width as i32)
            .width(width as u32)
            .height(height as u32)
            .border_width(0);
        conn.configure_window(client, &client_aux)?;

        // Track the mapping
        self.client_to_frame.insert(client, frame);
        self.frame_to_client.insert(frame, client);

        tracing::debug!(
            "Created border frame {} for client {} at ({}, {}) size {}x{} (border: {})",
            frame, client, x, y, frame_width, frame_height, border_width
        );

        Ok(frame)
    }

    /// Configure a border frame's geometry.
    pub fn configure_border_frame<C: X11Connection>(
        &self,
        conn: &C,
        client: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
    ) -> Result<(), Error> {
        if let Some(&frame) = self.client_to_frame.get(&client) {
            let frame_width = width + border_width * 2;
            let frame_height = height + border_width * 2;

            // Configure frame position and size
            let frame_aux = ConfigureWindowAux::new()
                .x(x as i32)
                .y(y as i32)
                .width(frame_width as u32)
                .height(frame_height as u32)
                .border_width(0);
            conn.configure_window(frame, &frame_aux)?;

            // Configure client within frame
            let client_aux = ConfigureWindowAux::new()
                .x(border_width as i32)
                .y(border_width as i32)
                .width(width as u32)
                .height(height as u32);
            conn.configure_window(client, &client_aux)?;
        }
        Ok(())
    }
}

/// Interpolate between two colors.
/// t should be in range 0.0 to 1.0.
fn interpolate_color(c1: u32, c2: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);

    let r1 = ((c1 >> 16) & 0xFF) as f32;
    let g1 = ((c1 >> 8) & 0xFF) as f32;
    let b1 = (c1 & 0xFF) as f32;

    let r2 = ((c2 >> 16) & 0xFF) as f32;
    let g2 = ((c2 >> 8) & 0xFF) as f32;
    let b2 = (c2 & 0xFF) as f32;

    let r = (r1 + (r2 - r1) * t) as u32;
    let g = (g1 + (g2 - g1) * t) as u32;
    let b = (b1 + (b2 - b1) * t) as u32;

    (r << 16) | (g << 8) | b
}

impl Default for FrameManager {
    fn default() -> Self {
        Self::new()
    }
}
