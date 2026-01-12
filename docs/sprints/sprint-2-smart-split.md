# Sprint 2: Smart Split + Keyboard Navigation

**Goal:** Implement the core differentiating feature (intelligent split direction) and full keyboard-based window navigation.

## Objectives

- Automatic split direction based on window dimensions
- Full keyboard navigation (vim-style hjkl)
- Keyboard-based window operations
- This is what makes gar different from i3

## Prerequisites

- Sprint 1 complete (BSP tree, basic window management)

## The Smart Split Algorithm

This is the key feature that Hyprland has and i3 lacks:

```
When inserting a new window:
1. Get dimensions of the target area (width, height)
2. If width > height → split Vertical (creates side-by-side windows)
3. If height >= width → split Horizontal (creates stacked windows)
```

**Visual example:**
```
┌─────────────────────────┐     ┌────────────┬────────────┐
│                         │     │            │            │
│    Wide container       │ --> │   Left     │   Right    │
│    (width > height)     │     │            │   (new)    │
│                         │     │            │            │
└─────────────────────────┘     └────────────┴────────────┘
       Split VERTICAL

┌───────────┐                   ┌───────────┐
│           │                   │   Top     │
│   Tall    │                   ├───────────┤
│ container │    ────────>      │  Bottom   │
│ (h >= w)  │                   │   (new)   │
│           │                   │           │
└───────────┘                   └───────────┘
       Split HORIZONTAL
```

## Tasks

### 2.1 Implement Smart Split
- [ ] Modify `insert_window` in tree.rs
- [ ] Calculate container dimensions before inserting
- [ ] Choose split direction based on width vs height
- [ ] Add unit tests for split direction logic

```rust
impl Node {
    fn determine_split_direction(rect: &Rect) -> SplitDirection {
        if rect.width > rect.height {
            SplitDirection::Vertical
        } else {
            SplitDirection::Horizontal
        }
    }

    pub fn insert_window(&mut self, window: WindowId, rect: Rect) {
        let direction = Self::determine_split_direction(&rect);
        // ... rest of insertion logic
    }
}
```

### 2.2 Keyboard Input Infrastructure
- [ ] Create `src/input/mod.rs` and `src/input/keybinds.rs`
- [ ] Define keybind action enum
- [ ] Create keybind registry
- [ ] Grab all configured keys on startup
- [ ] Dispatch to actions on KeyPress event

```rust
pub enum Action {
    FocusDirection(Direction),
    SwapDirection(Direction),
    ResizeDirection(Direction, i32),
    CloseWindow,
    Equalize,
    Exec(String),
}

pub enum Direction {
    Left, Right, Up, Down,
}
```

### 2.3 Focus Navigation
- [ ] Implement directional focus (Mod+h/j/k/l)
- [ ] Find window in given direction from focused
- [ ] Handle edge cases (no window in direction)
- [ ] Update focus and visual feedback

**Algorithm for finding window in direction:**
```rust
fn find_window_in_direction(
    tree: &Node,
    from: WindowId,
    direction: Direction,
) -> Option<WindowId> {
    // 1. Get geometry of 'from' window
    // 2. Get geometries of all windows
    // 3. Filter to windows in the given direction
    // 4. Return closest window in that direction
}
```

### 2.4 Window Closing
- [ ] Implement Mod+Shift+Q to close focused window
- [ ] Send WM_DELETE_WINDOW if supported (ICCCM)
- [ ] Fall back to XKillClient if not
- [ ] Remove from tree after close

```rust
fn close_window(conn: &impl Connection, window: Window) -> Result<()> {
    // Check if window supports WM_DELETE_WINDOW
    if supports_delete_window(conn, window)? {
        send_delete_window_message(conn, window)?;
    } else {
        conn.kill_client(window)?;
    }
    Ok(())
}
```

### 2.5 Resize Splits
- [ ] Implement Mod+Ctrl+h/j/k/l to resize
- [ ] Find the split that affects the focused window in that direction
- [ ] Adjust ratio by fixed increment (e.g., 0.05)
- [ ] Clamp ratio between 0.1 and 0.9
- [ ] Recalculate and apply geometries

```rust
fn resize_in_direction(tree: &mut Node, window: WindowId, dir: Direction, delta: f32) {
    // Find the nearest ancestor split in the given direction
    // Adjust its ratio
}
```

### 2.6 Equalize Splits
- [ ] Implement Mod+E to equalize
- [ ] Set all split ratios to 0.5
- [ ] Recursive traversal of tree
- [ ] Recalculate and apply geometries

```rust
fn equalize(node: &mut Node) {
    match node {
        Node::Internal { ratio, left, right, .. } => {
            *ratio = 0.5;
            equalize(left);
            equalize(right);
        }
        Node::Leaf { .. } => {}
    }
}
```

### 2.7 Swap Windows
- [ ] Implement Mod+Shift+h/j/k/l to swap
- [ ] Find window in direction
- [ ] Swap the two windows in the tree
- [ ] Recalculate and apply geometries

```rust
fn swap_windows(tree: &mut Node, a: WindowId, b: WindowId) {
    // Find both leaf nodes
    // Swap their window IDs
}
```

## Keybind Summary

| Keybind | Action |
|---------|--------|
| Mod+h/j/k/l | Focus left/down/up/right |
| Mod+Shift+h/j/k/l | Swap with left/down/up/right |
| Mod+Ctrl+h/j/k/l | Resize split in direction |
| Mod+Shift+Q | Close focused window |
| Mod+E | Equalize all splits |
| Mod+Return | Spawn terminal |

## Acceptance Criteria

1. New windows split intelligently based on container shape
2. All keyboard navigation works as specified
3. Window swapping maintains tree structure
4. Resize adjusts the correct split
5. Equalize makes all windows same size
6. Closing windows works cleanly

## Testing Strategy

```bash
# Test smart splits
# Open terminal in empty workspace → full screen
# Open second terminal → should split vertically (side by side)
# Focus right window, open third → right window splits horizontally (stacked)

# Test navigation
# Mod+h/l should move focus left/right
# Mod+j/k should move focus up/down

# Test resize
# Mod+Ctrl+l should make focused window wider
# Mod+E should reset to equal sizes
```

## Notes

- This sprint establishes the core UX that differentiates gar
- Navigation should feel snappy (no perceptible delay)
- Consider adding Mod+Arrow keys as alternative to hjkl
- Direction finding algorithm is the trickiest part - test thoroughly
