# Sprint 5: Floating Windows

**Goal:** Full floating window support with mouse interaction.

## Objectives

- Toggle windows between tiled and floating
- Mouse-based move and resize for floating windows
- Automatic floating for dialogs and certain window types
- Proper stacking order (floating above tiled)

## Prerequisites

- Sprint 4 complete (Lua configuration)

## Tasks

### 5.1 Floating Window State
- [ ] Add floating window list to Workspace
- [ ] Track floating window geometry separately
- [ ] Floating windows don't participate in BSP tree
- [ ] Track stacking order for floating windows

```rust
pub struct Workspace {
    pub tree: Node,                    // Tiled windows
    pub floating: Vec<FloatingWindow>, // Floating windows
    pub focused: Option<WindowId>,
    // ...
}

pub struct FloatingWindow {
    pub id: WindowId,
    pub geometry: Rect,
    pub z_order: u32,
}
```

### 5.2 Toggle Floating
- [ ] Implement Mod+Shift+Space to toggle
- [ ] When floating: remove from tree, add to floating list
- [ ] When tiling: remove from floating list, insert to tree
- [ ] Preserve position when floating
- [ ] Use smart split when returning to tiled

```rust
fn toggle_floating(&mut self, window: WindowId) {
    if self.is_floating(window) {
        // Move to tiled
        let float_win = self.floating.remove(/* ... */);
        self.tree.insert(window);
    } else {
        // Move to floating
        let geometry = self.get_window_geometry(window);
        self.tree.remove(window);
        self.floating.push(FloatingWindow {
            id: window,
            geometry,
            z_order: self.next_z_order(),
        });
    }
}
```

### 5.3 Mouse Move (Mod+LeftClick)
- [ ] Grab Mod+Button1 on all windows
- [ ] On press: record start position
- [ ] On motion: update window position
- [ ] On release: finalize position
- [ ] Only works for floating windows

```rust
fn handle_button_press(&mut self, event: ButtonPressEvent) {
    if event.detail == 1 && event.state.contains(ModMask::M4) {
        // Start move operation
        self.drag_state = Some(DragState::Move {
            window: event.child,
            start_x: event.root_x,
            start_y: event.root_y,
            start_geometry: self.get_geometry(event.child),
        });
        self.conn.grab_pointer(/* ... */)?;
    }
}

fn handle_motion(&mut self, event: MotionNotifyEvent) {
    if let Some(DragState::Move { window, start_x, start_y, start_geometry }) = &self.drag_state {
        let dx = event.root_x - start_x;
        let dy = event.root_y - start_y;
        let new_x = start_geometry.x + dx;
        let new_y = start_geometry.y + dy;
        self.configure_window(window, new_x, new_y, None, None)?;
    }
}
```

### 5.4 Mouse Resize (Mod+RightClick)
- [ ] Grab Mod+Button3 on all windows
- [ ] Determine resize corner based on click position
- [ ] On motion: adjust size (and position for top/left edges)
- [ ] Enforce minimum window size
- [ ] Only works for floating windows

```rust
fn determine_resize_edge(window_geometry: Rect, click_x: i16, click_y: i16) -> ResizeEdge {
    let center_x = window_geometry.x + window_geometry.width as i16 / 2;
    let center_y = window_geometry.y + window_geometry.height as i16 / 2;

    match (click_x < center_x, click_y < center_y) {
        (true, true) => ResizeEdge::TopLeft,
        (false, true) => ResizeEdge::TopRight,
        (true, false) => ResizeEdge::BottomLeft,
        (false, false) => ResizeEdge::BottomRight,
    }
}
```

### 5.5 Auto-Float Rules
- [ ] Float `_NET_WM_WINDOW_TYPE_DIALOG`
- [ ] Float `_NET_WM_WINDOW_TYPE_SPLASH`
- [ ] Float `_NET_WM_WINDOW_TYPE_TOOLTIP`
- [ ] Float `_NET_WM_WINDOW_TYPE_UTILITY`
- [ ] Float windows with WM_TRANSIENT_FOR set
- [ ] Respect size hints for floating position

```rust
fn should_float(conn: &impl Connection, window: Window) -> Result<bool> {
    let window_type = get_window_type(conn, window)?;

    let float_types = [
        atoms._NET_WM_WINDOW_TYPE_DIALOG,
        atoms._NET_WM_WINDOW_TYPE_SPLASH,
        atoms._NET_WM_WINDOW_TYPE_TOOLTIP,
        atoms._NET_WM_WINDOW_TYPE_UTILITY,
        atoms._NET_WM_WINDOW_TYPE_POPUP_MENU,
    ];

    if float_types.contains(&window_type) {
        return Ok(true);
    }

    // Check transient
    if get_transient_for(conn, window)?.is_some() {
        return Ok(true);
    }

    Ok(false)
}
```

### 5.6 Stacking Order
- [ ] Floating windows above tiled
- [ ] Most recently focused floating on top
- [ ] Use `ConfigureWindow` with `stack_mode` and `sibling`
- [ ] Maintain stacking order on focus changes

```rust
fn raise_window(&self, conn: &impl Connection, window: Window) -> Result<()> {
    conn.configure_window(
        window,
        &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    )?;
    Ok(())
}

fn restack_floating(&self, conn: &impl Connection) -> Result<()> {
    // Stack in z_order, lowest first
    let mut sorted: Vec<_> = self.floating.iter().collect();
    sorted.sort_by_key(|w| w.z_order);

    for (i, window) in sorted.iter().enumerate() {
        if i > 0 {
            conn.configure_window(
                window.id,
                &ConfigureWindowAux::new()
                    .stack_mode(StackMode::ABOVE)
                    .sibling(sorted[i - 1].id),
            )?;
        }
    }
    Ok(())
}
```

### 5.7 Floating Focus
- [ ] Tab cycles through floating windows
- [ ] Focus follows click for floating
- [ ] Raise on focus
- [ ] Handle focus between tiled and floating

```rust
fn focus_next_floating(&mut self) {
    if self.floating.is_empty() {
        return;
    }

    let current_idx = self.floating
        .iter()
        .position(|w| Some(w.id) == self.focused)
        .unwrap_or(0);

    let next_idx = (current_idx + 1) % self.floating.len();
    let next_window = self.floating[next_idx].id;

    self.set_focus(next_window);
    self.raise_window(next_window);
}
```

### 5.8 Lua Integration
- [ ] Add `floating` property to window rules
- [ ] Add `gar.toggle_floating()` function
- [ ] Add `gar.float_geometry(x, y, w, h)` for rules

```lua
-- Float Firefox PiP
gar.rule(
    { class = "firefox", title = "Picture-in-Picture" },
    { floating = true, geometry = { x = 100, y = 100, width = 400, height = 300 } }
)

-- Keybind
gar.bind("mod+shift+space", gar.toggle_floating)
```

## Keybind Summary

| Keybind | Action |
|---------|--------|
| Mod+Shift+Space | Toggle floating |
| Mod+LeftClick drag | Move floating window |
| Mod+RightClick drag | Resize floating window |

## Acceptance Criteria

1. Mod+Shift+Space toggles between tiled/floating
2. Can drag floating windows with Mod+LeftClick
3. Can resize floating windows with Mod+RightClick
4. Dialogs automatically float
5. Floating windows stay above tiled
6. Focus works correctly between tiled/floating

## Testing Strategy

```bash
# Test toggle
# Open xterm, verify tiled
# Mod+Shift+Space, verify floating (doesn't affect layout)
# Mod+Shift+Space again, verify back in tile

# Test mouse
# Float a window
# Mod+LeftClick drag - should move
# Mod+RightClick drag - should resize

# Test auto-float
# Open a dialog (e.g., Firefox preferences)
# Should automatically float

# Test stacking
# Have tiled windows
# Float one
# Floating should be on top
```

## Notes

- Consider minimum size constraints
- Handle fullscreen windows (Sprint 8)
- Floating geometry should be remembered when toggling
- Resize should work from any edge (not just corner)
