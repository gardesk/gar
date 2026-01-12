# Sprint 7: Multi-Monitor Support

**Goal:** Proper multi-head support with per-monitor workspaces.

## Objectives

- Detect monitors via RandR extension
- Assign workspaces to monitors
- Navigate and move windows between monitors
- Handle monitor hotplug (connect/disconnect)

## Prerequisites

- Sprint 6 complete (IPC system)

## Monitor Model

```
┌─────────────────────────────────────────────────────────────────────┐
│                         WorkspaceManager                             │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐         │
│  │  Monitor 0   │     │  Monitor 1   │     │  Monitor 2   │         │
│  │  "eDP-1"     │     │  "HDMI-1"    │     │  "DP-1"      │         │
│  │              │     │              │     │              │         │
│  │  WS: 1,2,3   │     │  WS: 4,5,6   │     │  WS: 7,8,9   │         │
│  │  Active: 1   │     │  Active: 4   │     │  Active: 7   │         │
│  └──────────────┘     └──────────────┘     └──────────────┘         │
└─────────────────────────────────────────────────────────────────────┘
```

## Tasks

### 7.1 Monitor Detection
- [ ] Create `src/core/monitor.rs`
- [ ] Query RandR for connected outputs
- [ ] Get monitor geometry (position, size)
- [ ] Identify primary monitor
- [ ] Handle disabled/mirrored outputs

```rust
use x11rb::protocol::randr;

pub struct Monitor {
    pub name: String,
    pub output: randr::Output,
    pub geometry: Rect,
    pub primary: bool,
    pub workspaces: Vec<WorkspaceId>,
    pub active_workspace: WorkspaceId,
}

fn detect_monitors(conn: &impl Connection, root: Window) -> Result<Vec<Monitor>> {
    let resources = conn.randr_get_screen_resources(root)?.reply()?;
    let mut monitors = Vec::new();

    for output in resources.outputs {
        let info = conn.randr_get_output_info(output, 0)?.reply()?;

        if info.connection != randr::Connection::CONNECTED {
            continue;
        }

        if let Some(crtc) = info.crtc {
            let crtc_info = conn.randr_get_crtc_info(crtc, 0)?.reply()?;
            monitors.push(Monitor {
                name: String::from_utf8_lossy(&info.name).to_string(),
                output,
                geometry: Rect {
                    x: crtc_info.x,
                    y: crtc_info.y,
                    width: crtc_info.width,
                    height: crtc_info.height,
                },
                primary: false, // Set later
                workspaces: Vec::new(),
                active_workspace: WorkspaceId(0),
            });
        }
    }

    Ok(monitors)
}
```

### 7.2 Workspace-Monitor Assignment
- [ ] Each monitor has a set of workspaces
- [ ] Default: distribute workspaces evenly
- [ ] Lua API: `gar.assign_workspace(ws, monitor)`
- [ ] Workspace shows on its assigned monitor

```rust
impl WorkspaceManager {
    fn assign_workspaces_to_monitors(&mut self) {
        let ws_per_monitor = self.workspaces.len() / self.monitors.len();

        for (i, monitor) in self.monitors.iter_mut().enumerate() {
            let start = i * ws_per_monitor + 1;
            let end = if i == self.monitors.len() - 1 {
                self.workspaces.len()
            } else {
                start + ws_per_monitor
            };

            monitor.workspaces = (start..=end).map(WorkspaceId).collect();
            monitor.active_workspace = WorkspaceId(start);
        }
    }
}
```

### 7.3 Focus Across Monitors
- [ ] Implement Mod+comma/period to focus monitor
- [ ] Track focused monitor
- [ ] Focus follows workspace on switch
- [ ] Handle case when monitor has no windows

```rust
fn focus_monitor(&mut self, direction: Direction) {
    let current_monitor = self.focused_monitor();
    let target = self.monitor_in_direction(current_monitor, direction)?;

    // Focus the active workspace on target monitor
    let workspace = self.monitors[target].active_workspace;
    if let Some(window) = self.workspaces[workspace].focused {
        self.set_focus(window);
    }

    self.focused_monitor = target;
}
```

### 7.4 Move Window to Monitor
- [ ] Implement Mod+Shift+comma/period
- [ ] Remove from current workspace
- [ ] Add to target monitor's active workspace
- [ ] Optionally follow window

```rust
fn move_to_monitor(&mut self, direction: Direction) {
    let window = self.focused_window()?;
    let target_monitor = self.monitor_in_direction(self.focused_monitor, direction)?;
    let target_workspace = self.monitors[target_monitor].active_workspace;

    // Move window
    self.current_workspace_mut().tree.remove(window);
    self.workspace_mut(target_workspace).tree.insert(window);

    // Update geometries
    self.apply_layouts()?;
}
```

### 7.5 Per-Monitor Workspace Switching
- [ ] Mod+N switches workspace on focused monitor
- [ ] Only switches within monitor's workspace set
- [ ] Or switches to workspace and focuses its monitor
- [ ] Configurable behavior via Lua

```rust
fn switch_workspace(&mut self, workspace: WorkspaceId) {
    // Find which monitor this workspace belongs to
    let target_monitor = self.monitors.iter()
        .position(|m| m.workspaces.contains(&workspace));

    match target_monitor {
        Some(monitor_idx) => {
            // Switch workspace on that monitor
            self.monitors[monitor_idx].active_workspace = workspace;

            // Focus that monitor
            self.focused_monitor = monitor_idx;

            // Update visibility
            self.update_workspace_visibility();
        }
        None => {
            tracing::warn!("Workspace {:?} not assigned to any monitor", workspace);
        }
    }
}
```

### 7.6 Monitor Hotplug
- [ ] Subscribe to RandR events
- [ ] Handle ScreenChangeNotify
- [ ] Detect added/removed monitors
- [ ] Reassign workspaces as needed
- [ ] Move windows from disconnected monitors

```rust
fn handle_randr_event(&mut self, event: randr::ScreenChangeNotifyEvent) {
    let new_monitors = detect_monitors(&self.conn, self.root)?;

    // Find removed monitors
    for old in &self.monitors {
        if !new_monitors.iter().any(|m| m.name == old.name) {
            // Monitor disconnected - move its workspaces/windows
            self.handle_monitor_removed(old);
        }
    }

    // Find added monitors
    for new in &new_monitors {
        if !self.monitors.iter().any(|m| m.name == new.name) {
            // Monitor connected - assign workspaces
            self.handle_monitor_added(new);
        }
    }

    self.monitors = new_monitors;
    self.apply_all_layouts()?;
}
```

### 7.7 EWMH Multi-Monitor
- [ ] Set `_NET_DESKTOP_GEOMETRY` (combined size)
- [ ] Set `_NET_DESKTOP_VIEWPORT` per workspace
- [ ] Set `_NET_WORKAREA` per monitor
- [ ] Update on monitor changes

### 7.8 Lua Configuration
- [ ] `gar.assign_workspace(ws, monitor_name)`
- [ ] `gar.focus_monitor(direction)` action
- [ ] `gar.move_to_monitor(direction)` action
- [ ] Configure focus-follows-workspace behavior

```lua
-- Assign specific workspaces to monitors
gar.assign_workspace(1, "eDP-1")
gar.assign_workspace(2, "eDP-1")
gar.assign_workspace(3, "HDMI-1")
gar.assign_workspace(4, "HDMI-1")

-- Keybinds
gar.bind("mod+comma", function() gar.focus_monitor("prev") end)
gar.bind("mod+period", function() gar.focus_monitor("next") end)
gar.bind("mod+shift+comma", function() gar.move_to_monitor("prev") end)
gar.bind("mod+shift+period", function() gar.move_to_monitor("next") end)
```

## Keybind Summary

| Keybind | Action |
|---------|--------|
| Mod+comma | Focus previous monitor |
| Mod+period | Focus next monitor |
| Mod+Shift+comma | Move window to previous monitor |
| Mod+Shift+period | Move window to next monitor |

## Acceptance Criteria

1. Multiple monitors detected correctly
2. Each monitor shows its own workspaces
3. Mod+comma/period moves focus between monitors
4. Mod+Shift+comma/period moves windows between monitors
5. Workspace switch on any monitor works
6. Hotplug works (monitor disconnect/reconnect)
7. Windows from disconnected monitor move to remaining

## Testing Strategy

```bash
# With multiple monitors (or Xephyr instances)
xrandr --listmonitors  # Verify detection

# Test focus
# Mod+period - focus should move to other monitor
# Mod+comma - focus should move back

# Test move
# Mod+Shift+period - window should move to other monitor

# Test hotplug (if possible)
# Disconnect monitor, verify workspaces/windows migrate
```

## Notes

- Consider "primary" monitor concept for default workspace
- Polybar needs per-monitor instances
- Handle Xinerama fallback for older setups?
- Monitor arrangement (which is "left"/"right") comes from xrandr
