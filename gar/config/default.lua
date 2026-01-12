-- gar default configuration
-- Copy to ~/.config/gar/init.lua to customize

-- Appearance
gar.set("border_width", 2)
gar.set("border_color_focused", "#5294e2")
gar.set("border_color_unfocused", "#2d2d2d")
gar.set("gap_inner", 8)
gar.set("gap_outer", 8)

-- Mod key: "mod" = Super/Win, "alt" = Alt
-- Use "mod" for real X session, "alt" for nested testing (Xephyr)
local mod = "mod"

-- Terminal
gar.bind(mod .. "+Return", function()
    gar.exec("alacritty || kitty || foot || xterm")
end)

-- Close window
gar.bind(mod .. "+q", gar.close_window)

-- Reload config
gar.bind(mod .. "+shift+r", gar.reload)

-- PANIC: Exit gar immediately (mod+shift+Escape)
gar.bind(mod .. "+shift+Escape", gar.exit)

-- Focus navigation (arrow keys)
gar.bind(mod .. "+Left", gar.focus("left"))
gar.bind(mod .. "+Right", gar.focus("right"))
gar.bind(mod .. "+Up", gar.focus("up"))
gar.bind(mod .. "+Down", gar.focus("down"))

-- Focus navigation (vim keys)
gar.bind(mod .. "+h", gar.focus("left"))
gar.bind(mod .. "+l", gar.focus("right"))
gar.bind(mod .. "+k", gar.focus("up"))
gar.bind(mod .. "+j", gar.focus("down"))

-- Swap windows
gar.bind(mod .. "+shift+Left", gar.swap("left"))
gar.bind(mod .. "+shift+Right", gar.swap("right"))
gar.bind(mod .. "+shift+Up", gar.swap("up"))
gar.bind(mod .. "+shift+Down", gar.swap("down"))

gar.bind(mod .. "+shift+h", gar.swap("left"))
gar.bind(mod .. "+shift+l", gar.swap("right"))
gar.bind(mod .. "+shift+k", gar.swap("up"))
gar.bind(mod .. "+shift+j", gar.swap("down"))

-- Resize
gar.bind(mod .. "+ctrl+Left", gar.resize("left", 0.05))
gar.bind(mod .. "+ctrl+Right", gar.resize("right", 0.05))
gar.bind(mod .. "+ctrl+Up", gar.resize("up", 0.05))
gar.bind(mod .. "+ctrl+Down", gar.resize("down", 0.05))

gar.bind(mod .. "+ctrl+h", gar.resize("left", 0.05))
gar.bind(mod .. "+ctrl+l", gar.resize("right", 0.05))
gar.bind(mod .. "+ctrl+k", gar.resize("up", 0.05))
gar.bind(mod .. "+ctrl+j", gar.resize("down", 0.05))

-- Equalize splits
gar.bind(mod .. "+e", gar.equalize)

-- Toggle floating
gar.bind(mod .. "+f", gar.toggle_floating)

-- Cycle through floating windows
gar.bind(mod .. "+Tab", gar.cycle_floating)

-- Workspaces
for i = 1, 9 do
    gar.bind(mod .. "+" .. i, gar.workspace(i))
    gar.bind(mod .. "+shift+" .. i, gar.move_to_workspace(i))
end
gar.bind(mod .. "+0", gar.workspace(10))
gar.bind(mod .. "+shift+0", gar.move_to_workspace(10))

-- Multi-monitor (comma/period = prev/next)
gar.bind(mod .. "+comma", gar.focus_monitor("prev"))
gar.bind(mod .. "+period", gar.focus_monitor("next"))
gar.bind(mod .. "+shift+comma", gar.move_to_monitor("prev"))
gar.bind(mod .. "+shift+period", gar.move_to_monitor("next"))
