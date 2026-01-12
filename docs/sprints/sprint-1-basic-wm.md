# Sprint 1: Basic Window Management

**Goal:** Manage windows in a single workspace with manual tiling using a BSP tree.

## Objectives

- Track window lifecycle (create, destroy)
- Implement Binary Space Partition tree data structure
- Tile windows using the BSP tree
- Basic focus handling (click-to-focus)
- One hardcoded keybind to test input grabbing

## Prerequisites

- Sprint 0 complete (X connection, event loop working)

## Tasks

### 1.1 Window Tracking
- [ ] Create `src/core/window.rs`
- [ ] Define `Window` struct to track window state
- [ ] Create window registry (HashMap<XWindow, Window>)
- [ ] Handle MapRequest: add window to registry
- [ ] Handle UnmapNotify: remove window from registry
- [ ] Handle DestroyNotify: clean up window

**Window struct:**
```rust
pub struct Window {
    pub id: x11rb::protocol::xproto::Window,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub mapped: bool,
}
```

### 1.2 BSP Tree Data Structure
- [ ] Create `src/core/tree.rs`
- [ ] Implement `SplitDirection` enum (Horizontal, Vertical)
- [ ] Implement `Node` enum (Internal, Leaf)
- [ ] Implement tree traversal methods
- [ ] Implement node insertion (always vertical for now)
- [ ] Implement node removal
- [ ] Implement geometry calculation from tree

**Core tree structure:**
```rust
#[derive(Debug, Clone, Copy)]
pub enum SplitDirection {
    Horizontal,  // Top/Bottom
    Vertical,    // Left/Right
}

#[derive(Debug)]
pub enum Node {
    Internal {
        split: SplitDirection,
        ratio: f32,
        left: Box<Node>,
        right: Box<Node>,
    },
    Leaf {
        window: Option<WindowId>,
    },
}
```

### 1.3 Geometry Calculation
- [ ] Calculate window geometries from BSP tree
- [ ] Pass root geometry (screen size minus any reserved space)
- [ ] Recursively divide space based on splits and ratios
- [ ] Return list of (WindowId, Rect) pairs

**Algorithm:**
```rust
impl Node {
    pub fn calculate_geometries(&self, rect: Rect) -> Vec<(WindowId, Rect)> {
        match self {
            Node::Leaf { window: Some(id) } => vec![(*id, rect)],
            Node::Leaf { window: None } => vec![],
            Node::Internal { split, ratio, left, right } => {
                let (left_rect, right_rect) = rect.split(*split, *ratio);
                let mut result = left.calculate_geometries(left_rect);
                result.extend(right.calculate_geometries(right_rect));
                result
            }
        }
    }
}
```

### 1.4 Apply Geometries to X11
- [ ] Create function to configure window geometry
- [ ] Use `ConfigureWindow` request
- [ ] Handle border width in calculations
- [ ] Apply all geometries after tree changes

```rust
fn apply_geometry(conn: &impl Connection, window: Window, rect: Rect) -> Result<()> {
    let aux = ConfigureWindowAux::new()
        .x(rect.x as i32)
        .y(rect.y as i32)
        .width(rect.width as u32)
        .height(rect.height as u32);
    conn.configure_window(window, &aux)?;
    Ok(())
}
```

### 1.5 Window Insertion
- [ ] Find the focused leaf node
- [ ] Insert new window next to focused window
- [ ] If no focused window, insert at root
- [ ] Split vertically (for now - smart split is Sprint 2)
- [ ] Recalculate and apply all geometries

### 1.6 Window Removal
- [ ] Find and remove window from tree
- [ ] Collapse parent if only one child remains
- [ ] Handle removing last window (empty tree)
- [ ] Update focus to sibling or parent's other child
- [ ] Recalculate and apply geometries

### 1.7 Focus Handling
- [ ] Track currently focused window
- [ ] Handle ButtonPress on windows (click-to-focus)
- [ ] Set input focus using `SetInputFocus`
- [ ] Visual feedback: different border color for focused window
- [ ] Grab button on unfocused windows, ungrab on focused

```rust
// Set focus
conn.set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;

// Visual feedback
let border_color = if focused { 0x5294e2 } else { 0x2d2d2d };
conn.change_window_attributes(window, &ChangeWindowAttributesAux::new()
    .border_pixel(border_color))?;
```

### 1.8 Basic Keyboard Grab
- [ ] Grab Mod4+Return (super+enter) globally
- [ ] On keypress, spawn terminal (hardcoded: `alacritty` or `xterm`)
- [ ] Use `std::process::Command` to spawn

```rust
// Grab the key
conn.grab_key(
    false,
    root,
    ModMask::M4,  // Super/Win key
    keycode_for_return,
    GrabMode::ASYNC,
    GrabMode::ASYNC,
)?;
```

## Acceptance Criteria

1. Opening windows tiles them vertically (side by side)
2. Closing a window removes it and re-tiles remaining windows
3. Clicking a window focuses it (border color changes)
4. Mod+Enter spawns a terminal
5. Windows properly fill available screen space
6. No crashes on rapid window open/close

## Testing Strategy

```bash
# In Xephyr session
DISPLAY=:1 cargo run &

# Open multiple terminals
DISPLAY=:1 xterm &
DISPLAY=:1 xterm &
DISPLAY=:1 xterm &

# Verify: 3 windows tiled vertically
# Click each to verify focus changes
# Close middle window, verify remaining 2 expand
```

## Edge Cases to Handle

- First window (empty tree)
- Last window closed (empty tree again)
- Rapid window creation
- Windows that set their own size (override redirect)
- Transient windows (handle in later sprint)

## Notes

- Keep split ratio at 0.5 for now (equal splits)
- Vertical-only splits for now (Sprint 2 adds smart splitting)
- No gaps yet (Sprint 8)
- Focused border color hardcoded (Sprint 4 makes configurable)
