# Sprint 8: EWMH Compliance + Polish

**Goal:** Full EWMH compliance for ecosystem integration, plus visual polish.

## Objectives

- Complete EWMH atom support for tools like polybar, rofi, dunst
- Proper ICCCM compliance
- Visual features: borders, gaps, optional title bars
- Urgency hints

## Prerequisites

- Sprint 7 complete (multi-monitor)

## EWMH Compliance

### Required Atoms

| Atom | Purpose | Implementation |
|------|---------|----------------|
| `_NET_SUPPORTED` | List of supported atoms | Set on root |
| `_NET_SUPPORTING_WM_CHECK` | WM identification | Child window |
| `_NET_CLIENT_LIST` | All managed windows | Update on changes |
| `_NET_CLIENT_LIST_STACKING` | Windows in stacking order | Update on changes |
| `_NET_NUMBER_OF_DESKTOPS` | Workspace count | Set on init |
| `_NET_DESKTOP_GEOMETRY` | Virtual desktop size | Set on init |
| `_NET_DESKTOP_VIEWPORT` | Viewport position | Per workspace |
| `_NET_CURRENT_DESKTOP` | Active workspace | Update on switch |
| `_NET_DESKTOP_NAMES` | Workspace names | Set on init |
| `_NET_ACTIVE_WINDOW` | Focused window | Update on focus |
| `_NET_WORKAREA` | Usable screen area | Per monitor |
| `_NET_WM_NAME` | WM name | "gar" |
| `_NET_WM_DESKTOP` | Window's workspace | Per window |
| `_NET_WM_STATE` | Window states | Per window |
| `_NET_WM_WINDOW_TYPE` | Window type | Read, respect |

## Tasks

### 8.1 EWMH Setup
- [ ] Create `src/x11/ewmh.rs`
- [ ] Intern all EWMH atoms on startup
- [ ] Create supporting WM check window
- [ ] Set `_NET_SUPPORTED` with all supported atoms
- [ ] Set WM name

```rust
pub struct EwmhAtoms {
    pub _NET_SUPPORTED: Atom,
    pub _NET_SUPPORTING_WM_CHECK: Atom,
    pub _NET_CLIENT_LIST: Atom,
    pub _NET_CLIENT_LIST_STACKING: Atom,
    pub _NET_NUMBER_OF_DESKTOPS: Atom,
    pub _NET_CURRENT_DESKTOP: Atom,
    pub _NET_DESKTOP_NAMES: Atom,
    pub _NET_ACTIVE_WINDOW: Atom,
    pub _NET_WM_NAME: Atom,
    pub _NET_WM_DESKTOP: Atom,
    pub _NET_WM_STATE: Atom,
    pub _NET_WM_STATE_FULLSCREEN: Atom,
    pub _NET_WM_STATE_HIDDEN: Atom,
    pub _NET_WM_STATE_DEMANDS_ATTENTION: Atom,
    pub _NET_WM_WINDOW_TYPE: Atom,
    pub _NET_WM_WINDOW_TYPE_DIALOG: Atom,
    // ... more
}

fn setup_ewmh(conn: &impl Connection, root: Window, atoms: &EwmhAtoms) -> Result<()> {
    // Create check window
    let check_window = conn.generate_id()?;
    conn.create_window(
        0, check_window, root,
        -1, -1, 1, 1, 0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new(),
    )?;

    // Set supporting WM check
    conn.change_property32(
        PropMode::REPLACE, root,
        atoms._NET_SUPPORTING_WM_CHECK,
        AtomEnum::WINDOW,
        &[check_window],
    )?;
    conn.change_property32(
        PropMode::REPLACE, check_window,
        atoms._NET_SUPPORTING_WM_CHECK,
        AtomEnum::WINDOW,
        &[check_window],
    )?;

    // Set WM name
    conn.change_property8(
        PropMode::REPLACE, check_window,
        atoms._NET_WM_NAME,
        atoms.UTF8_STRING,
        b"gar",
    )?;

    // Set supported atoms
    let supported = vec![
        atoms._NET_SUPPORTED,
        atoms._NET_SUPPORTING_WM_CHECK,
        // ... all supported atoms
    ];
    conn.change_property32(
        PropMode::REPLACE, root,
        atoms._NET_SUPPORTED,
        AtomEnum::ATOM,
        &supported,
    )?;

    Ok(())
}
```

### 8.2 Client List Maintenance
- [ ] Update `_NET_CLIENT_LIST` on window add/remove
- [ ] Update `_NET_CLIENT_LIST_STACKING` on stacking changes
- [ ] Update `_NET_ACTIVE_WINDOW` on focus change

```rust
fn update_client_list(&self, conn: &impl Connection) -> Result<()> {
    let windows: Vec<u32> = self.all_windows()
        .iter()
        .map(|w| w.id)
        .collect();

    conn.change_property32(
        PropMode::REPLACE,
        self.root,
        self.atoms._NET_CLIENT_LIST,
        AtomEnum::WINDOW,
        &windows,
    )?;

    Ok(())
}
```

### 8.3 Handle Client Messages
- [ ] Handle `_NET_ACTIVE_WINDOW` (focus requests from apps)
- [ ] Handle `_NET_WM_STATE` changes (fullscreen, etc.)
- [ ] Handle `_NET_CURRENT_DESKTOP` (pager workspace switch)
- [ ] Handle `_NET_CLOSE_WINDOW` (close requests)

```rust
fn handle_client_message(&mut self, event: ClientMessageEvent) -> Result<()> {
    let atom = event.type_;

    if atom == self.atoms._NET_ACTIVE_WINDOW {
        // Application requesting focus
        let window = event.window;
        self.focus_window(window)?;
    } else if atom == self.atoms._NET_WM_STATE {
        // State change request (e.g., fullscreen)
        let action = event.data.as_data32()[0];
        let prop = event.data.as_data32()[1];
        self.handle_state_change(event.window, action, prop)?;
    } else if atom == self.atoms._NET_CURRENT_DESKTOP {
        // Workspace switch request
        let desktop = event.data.as_data32()[0] as usize;
        self.switch_workspace(WorkspaceId(desktop + 1))?;
    }

    Ok(())
}
```

### 8.4 Window State Management
- [ ] Track window states (fullscreen, hidden, urgent)
- [ ] Update `_NET_WM_STATE` property
- [ ] Implement fullscreen toggle (Mod+F)
- [ ] Handle fullscreen requests from applications

```rust
fn set_fullscreen(&mut self, window: WindowId, fullscreen: bool) -> Result<()> {
    let win = self.get_window_mut(window)?;

    if fullscreen {
        // Save current geometry
        win.saved_geometry = Some(win.geometry);
        // Set to monitor size
        let monitor = self.get_monitor_for_window(window);
        win.geometry = monitor.geometry;
        win.states.insert(WindowState::Fullscreen);
    } else {
        // Restore geometry
        if let Some(saved) = win.saved_geometry.take() {
            win.geometry = saved;
        }
        win.states.remove(&WindowState::Fullscreen);
    }

    // Update X property
    self.update_wm_state(window)?;
    self.apply_geometry(window)?;

    Ok(())
}
```

### 8.5 ICCCM Compliance
- [ ] Create `src/x11/icccm.rs`
- [ ] Handle `WM_HINTS` (urgency, input, etc.)
- [ ] Handle `WM_NORMAL_HINTS` (size hints)
- [ ] Handle `WM_PROTOCOLS` (delete window, take focus)
- [ ] Handle `WM_TRANSIENT_FOR`

```rust
fn get_wm_hints(conn: &impl Connection, window: Window) -> Result<Option<WmHints>> {
    // Read WM_HINTS property
    // Parse into struct
}

fn get_size_hints(conn: &impl Connection, window: Window) -> Result<Option<SizeHints>> {
    // Read WM_NORMAL_HINTS
    // Extract min/max size, aspect ratio, etc.
}
```

### 8.6 Border Colors
- [ ] Configurable border colors via Lua
- [ ] Different colors: focused, unfocused, urgent
- [ ] Apply colors on focus change
- [ ] Flash urgent windows

```rust
fn update_border_color(&self, conn: &impl Connection, window: WindowId) -> Result<()> {
    let win = self.get_window(window)?;
    let color = if win.urgent {
        self.config.border_color_urgent
    } else if Some(window) == self.focused {
        self.config.border_color_focused
    } else {
        self.config.border_color_unfocused
    };

    conn.change_window_attributes(
        window,
        &ChangeWindowAttributesAux::new().border_pixel(color),
    )?;

    Ok(())
}
```

### 8.7 Gaps
- [ ] Inner gaps (between windows)
- [ ] Outer gaps (screen edge)
- [ ] Configurable via Lua
- [ ] Apply in geometry calculation

```rust
fn calculate_geometries_with_gaps(&self, rect: Rect, gaps: &GapConfig) -> Vec<(WindowId, Rect)> {
    // Shrink rect by outer gaps
    let work_area = Rect {
        x: rect.x + gaps.outer as i16,
        y: rect.y + gaps.outer as i16,
        width: rect.width - 2 * gaps.outer,
        height: rect.height - 2 * gaps.outer,
    };

    // Calculate from tree
    let mut geometries = self.tree.calculate_geometries(work_area);

    // Apply inner gaps
    for (_, geom) in &mut geometries {
        geom.x += gaps.inner as i16 / 2;
        geom.y += gaps.inner as i16 / 2;
        geom.width -= gaps.inner;
        geom.height -= gaps.inner;
    }

    geometries
}
```

### 8.8 Optional Title Bars
- [ ] Create title bar windows (reparenting)
- [ ] Draw window title
- [ ] Handle title bar clicks (focus, move)
- [ ] Configurable per window class
- [ ] Double-click to toggle maximize

```rust
fn create_frame(&mut self, conn: &impl Connection, window: WindowId) -> Result<Frame> {
    let title_height = if self.should_have_titlebar(window) {
        self.config.titlebar_height
    } else {
        0
    };

    // Create frame window
    let frame = conn.generate_id()?;
    conn.create_window(/* ... */)?;

    // Reparent client into frame
    conn.reparent_window(window, frame, 0, title_height)?;

    Ok(Frame { id: frame, client: window, title_height })
}
```

### 8.9 Urgency Hints
- [ ] Read `WM_HINTS` urgency flag
- [ ] Read `_NET_WM_STATE_DEMANDS_ATTENTION`
- [ ] Set urgent border color
- [ ] Indicate urgent workspace (for status bars)
- [ ] Clear urgency on focus

```rust
fn handle_urgency(&mut self, window: WindowId) -> Result<()> {
    let hints = get_wm_hints(&self.conn, window)?;

    if hints.map(|h| h.urgent).unwrap_or(false) {
        let win = self.get_window_mut(window)?;
        win.urgent = true;

        // Update workspace urgency
        let ws = self.get_workspace_for_window(window)?;
        self.workspaces[ws].urgent = true;

        // Update EWMH
        self.set_wm_state(window, StateAction::Add, self.atoms._NET_WM_STATE_DEMANDS_ATTENTION)?;

        // Update border
        self.update_border_color(window)?;
    }

    Ok(())
}
```

### 8.10 External Compositor Integration

**Goal:** Support external compositors (picom) for proper screen repainting and visual effects.

#### Background

Like i3, gar is NOT a compositing window manager. We rely on external compositors (picom, compton, xcompmgr) for:
- Proper screen repainting when windows close
- Transparency and visual effects
- Vsync and tear-free rendering

Without a compositor, X11 does not automatically repaint exposed areas when windows close, leading to visual artifacts (old window pixels remaining on screen).

#### Tasks
- [ ] Add picom launch to `gar-session.sh` (before gar starts)
- [ ] Add picom launch to `start-gar.sh` xinitrc
- [ ] Keep `clear_root_area()` as fallback for compositor-less setups
- [ ] Add `_NET_WM_BYPASS_COMPOSITOR` atom (for fullscreen apps to request un-redirection)

#### Session Script Changes
```bash
# Launch compositor before WM
picom -b --use-ewmh-active-win &
sleep 0.1  # Brief pause to let compositor initialize
```

#### Recommended Picom Flags
- `-b` or `--daemon`: Run as background daemon
- `--use-ewmh-active-win`: Use `_NET_ACTIVE_WINDOW` for focus (more reliable with tiling WMs)
- `--backend glx`: Better for screen tearing prevention (optional)

#### EWMH Atoms for Compositors
| Atom | Purpose | Who Sets It |
|------|---------|-------------|
| `_NET_WM_BYPASS_COMPOSITOR` | Un-redirect fullscreen windows | Apps (games, videos) |
| `_NET_ACTIVE_WINDOW` | Currently focused window | WM (gar) |
| `_NET_WM_OPACITY` | Window opacity hint | Apps or user tools |

#### Fallback Behavior
The `clear_root_area()` function remains as a fallback for users who don't have picom installed. When a compositor is running, it handles repainting automatically and `clear_root_area()` becomes a no-op in effect.

#### Acceptance Criteria
1. Windows close without visual artifacts when picom is running
2. No screen tearing during window operations
3. Fallback still works when picom is not installed (uses `clear_root_area()`)
4. Fullscreen apps (games, videos) can bypass compositor via `_NET_WM_BYPASS_COMPOSITOR`

---

## Keybind Summary

| Keybind | Action |
|---------|--------|
| Mod+F | Toggle fullscreen |

## Acceptance Criteria

1. Polybar shows workspaces correctly
2. Rofi can list and switch windows
3. Dunst notifications work properly
4. Applications can request fullscreen
5. Borders and gaps configurable and working
6. Urgent windows highlighted
7. No errors from EWMH-compliant applications

## Testing Strategy

```bash
# Polybar
# Install polybar, configure xworkspaces module
# Workspaces should display, clicking should switch

# Rofi
rofi -show window  # Should list all windows
# Selecting window should focus it

# Fullscreen
# Open Firefox, press F11
# Should go fullscreen correctly
# Press F11 again, should restore

# Urgency
# Set urgent hint on a window
# Border should change, workspace should indicate
```

## Notes

- EWMH spec: https://specifications.freedesktop.org/wm-spec/latest/
- ICCCM spec: https://tronche.com/gui/x/icccm/
- Title bars add complexity - consider making opt-in
- Test with various applications (Firefox, Chromium, mpv, etc.)
