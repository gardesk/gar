# Sprint 3: Multiple Workspaces

**Goal:** Implement named workspaces with switching and window movement.

## Objectives

- Multiple independent workspaces, each with its own BSP tree
- Keyboard-based workspace switching
- Move windows between workspaces
- EWMH workspace properties for status bar integration

## Prerequisites

- Sprint 2 complete (smart split, keyboard navigation)

## Tasks

### 3.1 Workspace Data Structure
- [ ] Create `src/core/workspace.rs`
- [ ] Define `Workspace` struct
- [ ] Create `WorkspaceManager` to hold all workspaces
- [ ] Initialize default workspaces (1-10)

```rust
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub tree: Node,
    pub floating: Vec<Window>,
    pub focused: Option<WindowId>,
    pub visible: bool,
}

pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active: WorkspaceId,
}
```

### 3.2 Workspace Initialization
- [ ] Create 10 default workspaces (named "1" through "10")
- [ ] Set workspace 1 as active on startup
- [ ] Existing windows go to workspace 1

```rust
impl WorkspaceManager {
    pub fn new() -> Self {
        let workspaces = (1..=10)
            .map(|i| Workspace {
                id: WorkspaceId(i),
                name: i.to_string(),
                tree: Node::empty(),
                floating: Vec::new(),
                focused: None,
                visible: i == 1,
            })
            .collect();
        Self { workspaces, active: WorkspaceId(1) }
    }
}
```

### 3.3 Workspace Switching
- [ ] Implement Mod+1 through Mod+9 (and Mod+0 for 10)
- [ ] Hide windows on old workspace
- [ ] Show windows on new workspace
- [ ] Restore focus to previously focused window
- [ ] Update EWMH _NET_CURRENT_DESKTOP

```rust
fn switch_workspace(&mut self, conn: &impl Connection, target: WorkspaceId) -> Result<()> {
    if target == self.active {
        return Ok(());
    }

    // Hide current workspace windows
    for window in self.current_workspace().all_windows() {
        conn.unmap_window(window)?;
    }

    // Show target workspace windows
    self.active = target;
    for window in self.current_workspace().all_windows() {
        conn.map_window(window)?;
    }

    // Restore focus
    if let Some(focused) = self.current_workspace().focused {
        set_focus(conn, focused)?;
    }

    Ok(())
}
```

### 3.4 Move Window to Workspace
- [ ] Implement Mod+Shift+1 through Mod+Shift+9
- [ ] Remove window from current workspace tree
- [ ] Add window to target workspace tree
- [ ] If target workspace is not visible, unmap window
- [ ] Optionally follow window to new workspace (configurable later)

```rust
fn move_to_workspace(
    &mut self,
    conn: &impl Connection,
    window: WindowId,
    target: WorkspaceId,
) -> Result<()> {
    // Remove from current
    self.current_workspace_mut().tree.remove(window);

    // Add to target
    self.workspace_mut(target).tree.insert(window);

    // Hide if target not visible
    if target != self.active {
        conn.unmap_window(window)?;
    }

    // Retile both workspaces
    self.apply_layout(conn, self.active)?;
    self.apply_layout(conn, target)?;

    Ok(())
}
```

### 3.5 Per-Workspace Trees
- [ ] Modify window insertion to use active workspace
- [ ] Modify window removal to find correct workspace
- [ ] Ensure tree operations target correct workspace
- [ ] Handle window destroy when on non-active workspace

### 3.6 EWMH Workspace Support
- [ ] Set `_NET_NUMBER_OF_DESKTOPS` on root
- [ ] Set `_NET_DESKTOP_NAMES` with workspace names
- [ ] Set `_NET_CURRENT_DESKTOP` when switching
- [ ] Set `_NET_WM_DESKTOP` on each window
- [ ] Handle `_NET_CURRENT_DESKTOP` client messages (pagers)

```rust
fn set_ewmh_workspace_hints(conn: &impl Connection, manager: &WorkspaceManager) -> Result<()> {
    let root = get_root(conn);

    // Number of desktops
    conn.change_property32(
        PropMode::REPLACE,
        root,
        atoms._NET_NUMBER_OF_DESKTOPS,
        AtomEnum::CARDINAL,
        &[manager.workspaces.len() as u32],
    )?;

    // Desktop names
    let names: Vec<u8> = manager.workspaces
        .iter()
        .flat_map(|ws| ws.name.bytes().chain(std::iter::once(0)))
        .collect();
    conn.change_property8(
        PropMode::REPLACE,
        root,
        atoms._NET_DESKTOP_NAMES,
        atoms.UTF8_STRING,
        &names,
    )?;

    // Current desktop
    conn.change_property32(
        PropMode::REPLACE,
        root,
        atoms._NET_CURRENT_DESKTOP,
        AtomEnum::CARDINAL,
        &[manager.active.0 as u32 - 1], // 0-indexed for EWMH
    )?;

    Ok(())
}
```

### 3.7 Handle Existing Windows on Startup
- [ ] Query existing windows on WM start
- [ ] Check `_NET_WM_DESKTOP` for previous workspace assignment
- [ ] Place windows in appropriate workspaces
- [ ] Map windows in active workspace only

## Keybind Summary

| Keybind | Action |
|---------|--------|
| Mod+1-9 | Switch to workspace 1-9 |
| Mod+0 | Switch to workspace 10 |
| Mod+Shift+1-9 | Move window to workspace 1-9 |
| Mod+Shift+0 | Move window to workspace 10 |

## Acceptance Criteria

1. Can switch between 10 workspaces
2. Each workspace maintains its own window layout
3. Moving window to another workspace works
4. Status bars (polybar) can display workspace info
5. Windows hidden/shown correctly on switch
6. Focus restored correctly when switching back

## Testing Strategy

```bash
# Test workspace switching
# Mod+1 - should be on workspace 1
# Open xterm
# Mod+2 - switch to workspace 2 (empty)
# Open xterm - should be alone
# Mod+1 - back to workspace 1, original xterm visible

# Test window movement
# On workspace 1 with xterm
# Mod+Shift+2 - moves xterm to workspace 2
# Mod+2 - verify xterm is there

# Test polybar integration
polybar example  # Should show workspace indicators
```

## Status Bar Integration

For polybar, create an i3-compatible module (we'll implement IPC in Sprint 6):

```ini
[module/workspaces]
type = internal/xworkspaces
label-active = %name%
label-occupied = %name%
label-empty = %name%
```

## Notes

- 0-indexed for EWMH, 1-indexed for user display
- Consider urgent workspace highlighting (Sprint 8)
- Named workspaces beyond 1-10 come with Lua config (Sprint 4)
- Multi-monitor workspace assignment comes in Sprint 7
