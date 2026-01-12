# Sprint 4: Lua Configuration

**Goal:** User-configurable keybinds, settings, and startup applications via Lua scripting.

## Objectives

- Load and execute Lua configuration file
- Expose gar API to Lua (keybinds, settings, rules)
- Support config hot-reload
- Provide sensible default configuration

## Prerequisites

- Sprint 3 complete (workspaces)

## Lua API Design

```lua
-- ~/.config/gar/init.lua

-- Settings
gar.set("border_width", 2)
gar.set("border_color_focused", "#5294e2")
gar.set("border_color_unfocused", "#2d2d2d")
gar.set("gap_inner", 5)
gar.set("gap_outer", 10)

-- Keybindings
gar.bind("mod+Return", function() gar.exec("alacritty") end)
gar.bind("mod+d", function() gar.exec("rofi -show drun") end)
gar.bind("mod+shift+q", gar.close_window)
gar.bind("mod+h", function() gar.focus("left") end)
gar.bind("mod+j", function() gar.focus("down") end)
gar.bind("mod+k", function() gar.focus("up") end)
gar.bind("mod+l", function() gar.focus("right") end)

-- Workspace bindings (loop)
for i = 1, 9 do
    gar.bind("mod+" .. i, function() gar.workspace(i) end)
    gar.bind("mod+shift+" .. i, function() gar.move_to_workspace(i) end)
end

-- Window rules
gar.rule({ class = "Firefox" }, { workspace = 2 })
gar.rule({ class = "Spotify" }, { floating = true })
gar.rule({ type = "dialog" }, { floating = true })

-- Startup applications
gar.exec_once("picom")
gar.exec_once("polybar")
gar.exec_once("dunst")
```

## Tasks

### 4.1 Lua Integration Setup
- [ ] Add `mlua` dependency
- [ ] Create `src/config/mod.rs` and `src/config/lua.rs`
- [ ] Initialize Lua state
- [ ] Create `gar` global table
- [ ] Handle Lua errors gracefully

```rust
use mlua::{Lua, Result as LuaResult};

pub struct LuaConfig {
    lua: Lua,
}

impl LuaConfig {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();

        // Create gar global table
        lua.globals().set("gar", lua.create_table()?)?;

        Ok(Self { lua })
    }
}
```

### 4.2 Configuration Loading
- [ ] Find config file (`~/.config/gar/init.lua`)
- [ ] Fall back to default config if not found
- [ ] Execute Lua file
- [ ] Handle syntax errors with helpful messages

```rust
impl LuaConfig {
    pub fn load(&self, wm: &mut WindowManager) -> Result<()> {
        let config_path = dirs::config_dir()
            .map(|p| p.join("gar/init.lua"))
            .filter(|p| p.exists());

        let source = match config_path {
            Some(path) => std::fs::read_to_string(path)?,
            None => include_str!("../../config/default.lua").to_string(),
        };

        self.lua.load(&source).exec()?;
        Ok(())
    }
}
```

### 4.3 Settings API
- [ ] Implement `gar.set(key, value)`
- [ ] Store settings in Rust-side config struct
- [ ] Support: border_width, border colors, gaps
- [ ] Validate values (types, ranges)

```rust
// Register gar.set
let set_fn = lua.create_function(|_, (key, value): (String, mlua::Value)| {
    // Send to config channel or store in shared state
    Ok(())
})?;
gar_table.set("set", set_fn)?;
```

**Supported settings:**
| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| border_width | int | 2 | Window border width in pixels |
| border_color_focused | string | "#5294e2" | Focused window border color |
| border_color_unfocused | string | "#2d2d2d" | Unfocused window border color |
| gap_inner | int | 0 | Gap between windows |
| gap_outer | int | 0 | Gap around screen edge |

### 4.4 Keybind API
- [ ] Implement `gar.bind(keyspec, callback)`
- [ ] Parse keyspec string ("mod+shift+q")
- [ ] Register callback function
- [ ] Grab keys via X11

```rust
// Key specification parser
fn parse_keyspec(spec: &str) -> Result<(Modifiers, Keycode)> {
    // "mod+shift+q" -> (MOD4 | SHIFT, keycode_q)
    let parts: Vec<&str> = spec.to_lowercase().split('+').collect();
    // ...
}
```

### 4.5 Built-in Actions
- [ ] `gar.focus(direction)` - focus in direction
- [ ] `gar.swap(direction)` - swap with direction
- [ ] `gar.resize(direction, amount)` - resize split
- [ ] `gar.close_window()` - close focused window
- [ ] `gar.workspace(n)` - switch workspace
- [ ] `gar.move_to_workspace(n)` - move window
- [ ] `gar.exec(cmd)` - spawn command
- [ ] `gar.exec_once(cmd)` - spawn only if not running
- [ ] `gar.reload()` - reload config

### 4.6 Window Rules
- [ ] Implement `gar.rule(match, actions)`
- [ ] Match by: class, instance, title, type
- [ ] Actions: workspace, floating, border, size, position
- [ ] Apply rules on window creation

```rust
pub struct WindowRule {
    pub match_class: Option<Regex>,
    pub match_instance: Option<Regex>,
    pub match_title: Option<Regex>,
    pub match_type: Option<WindowType>,

    pub workspace: Option<WorkspaceId>,
    pub floating: Option<bool>,
    pub border_width: Option<u32>,
    // ...
}
```

### 4.7 Config Reload
- [ ] Implement `gar.reload()` function
- [ ] Bind to Mod+Shift+R by default
- [ ] Clear existing keybinds
- [ ] Re-execute config file
- [ ] Apply new settings immediately
- [ ] Report errors without crashing

### 4.8 Default Configuration
- [ ] Create `config/default.lua`
- [ ] Include sensible defaults for all settings
- [ ] Include essential keybinds (focus, close, workspaces)
- [ ] Document all options in comments

## Default Configuration

```lua
-- config/default.lua
-- Default gar configuration

-- Appearance
gar.set("border_width", 2)
gar.set("border_color_focused", "#5294e2")
gar.set("border_color_unfocused", "#2d2d2d")
gar.set("gap_inner", 0)
gar.set("gap_outer", 0)

-- Mod key (mod = Super/Win key)
local mod = "mod"

-- Core keybindings
gar.bind(mod .. "+Return", function() gar.exec("xterm") end)
gar.bind(mod .. "+shift+q", gar.close_window)
gar.bind(mod .. "+shift+r", gar.reload)
gar.bind(mod .. "+shift+e", gar.exit)

-- Focus navigation
gar.bind(mod .. "+h", function() gar.focus("left") end)
gar.bind(mod .. "+j", function() gar.focus("down") end)
gar.bind(mod .. "+k", function() gar.focus("up") end)
gar.bind(mod .. "+l", function() gar.focus("right") end)

-- Window movement
gar.bind(mod .. "+shift+h", function() gar.swap("left") end)
gar.bind(mod .. "+shift+j", function() gar.swap("down") end)
gar.bind(mod .. "+shift+k", function() gar.swap("up") end)
gar.bind(mod .. "+shift+l", function() gar.swap("right") end)

-- Resize
gar.bind(mod .. "+ctrl+h", function() gar.resize("left", 0.05) end)
gar.bind(mod .. "+ctrl+j", function() gar.resize("down", 0.05) end)
gar.bind(mod .. "+ctrl+k", function() gar.resize("up", 0.05) end)
gar.bind(mod .. "+ctrl+l", function() gar.resize("right", 0.05) end)
gar.bind(mod .. "+e", gar.equalize)

-- Workspaces
for i = 1, 9 do
    gar.bind(mod .. "+" .. i, function() gar.workspace(i) end)
    gar.bind(mod .. "+shift+" .. i, function() gar.move_to_workspace(i) end)
end
gar.bind(mod .. "+0", function() gar.workspace(10) end)
gar.bind(mod .. "+shift+0", function() gar.move_to_workspace(10) end)

-- Window rules
gar.rule({ type = "dialog" }, { floating = true })
gar.rule({ type = "splash" }, { floating = true })
```

## Acceptance Criteria

1. Config loads from `~/.config/gar/init.lua`
2. Falls back to default if no config exists
3. All keybinds configurable via Lua
4. Settings (borders, gaps) apply immediately
5. Window rules work for class/type matching
6. Mod+Shift+R reloads config without restart
7. Syntax errors reported clearly, don't crash WM

## Testing Strategy

```bash
# Create test config
mkdir -p ~/.config/gar
cat > ~/.config/gar/init.lua << 'EOF'
gar.set("border_width", 4)
gar.set("border_color_focused", "#ff0000")
gar.bind("mod+t", function() print("test") end)
EOF

# Start gar, verify:
# - 4px borders
# - Red focus color
# - Mod+t prints to log

# Test reload
# Modify config, Mod+Shift+R, verify changes apply
```

## Notes

- Lua 5.4 via mlua with vendored feature (no system dependency)
- Consider sandboxing Lua (disable os.execute, io.*, etc.)
- exec_once needs to track spawned processes
- Error messages should include line numbers
