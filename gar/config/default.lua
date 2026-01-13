-- gar default configuration
-- Copy to ~/.config/gar/init.lua to customize

-- Autostart applications (only run once per session)
-- Uncomment the ones you want:
-- gar.exec_once("polybar")                    -- Status bar
-- gar.exec_once("picom")                      -- Compositor (for transparency/shadows)
-- gar.exec_once("dunst")                      -- Notification daemon
-- gar.exec_once("nm-applet")                  -- NetworkManager tray icon
-- gar.exec_once("blueman-applet")             -- Bluetooth tray icon
-- gar.exec_once("feh --bg-scale ~/wallpaper.jpg")  -- Wallpaper
-- gar.exec_once("xss-lock -- i3lock -c 000000")    -- Auto-lock on suspend

-- Appearance
gar.set("border_width", 2)
gar.set("border_color_focused", "#5294e2")
gar.set("border_color_unfocused", "#2d2d2d")
gar.set("gap_inner", 8)
gar.set("gap_outer", 8)

-- Visual Effects (picom compositor)
-- gar auto-generates ~/.config/gar/picom.conf from these settings
-- Changes take effect on reload (Mod+Shift+R) - picom is signaled automatically
-- gar.set("corner_radius", 12)              -- 0 = square corners
-- gar.set("blur_enabled", true)
-- gar.set("blur_method", "dual_kawase")     -- "gaussian", "dual_kawase", "box"
-- gar.set("blur_strength", 5)               -- 1-20 for dual_kawase
-- gar.set("shadow_enabled", true)
-- gar.set("shadow_radius", 12)
-- gar.set("shadow_opacity", 0.75)
-- gar.set("shadow_offset_x", -7)
-- gar.set("shadow_offset_y", -7)
-- gar.set("opacity_focused", 1.0)
-- gar.set("opacity_unfocused", 0.9)         -- Dim unfocused windows
-- gar.set("fade_enabled", true)
-- gar.set("fade_delta", 10)

-- Title bars (disabled by default)
-- gar.set("titlebar_enabled", true)
-- gar.set("titlebar_height", 20)
-- gar.set("titlebar_color_focused", "#3d3d3d")
-- gar.set("titlebar_color_unfocused", "#2d2d2d")
-- gar.set("titlebar_text_color", "#ffffff")

-- Border gradients (requires frame windows, disabled by default)
-- gar.set("border_gradient_enabled", true)
-- gar.set("border_gradient_start_focused", "#5294e2")
-- gar.set("border_gradient_end_focused", "#1a5fb4")
-- gar.set("border_gradient_start_unfocused", "#3d3d3d")
-- gar.set("border_gradient_end_unfocused", "#1d1d1d")
-- gar.set("border_gradient_direction", "vertical")  -- "vertical", "horizontal", "diagonal"

-- Animations (picom v12+)
-- open options: "slide-in", "fly-in", "appear", "none"
-- close options: "slide-out", "fly-out", "disappear", "none"
-- gar.set("animation_open", "fly-in")
-- gar.set("animation_close", "fly-out")
-- gar.set("animation_duration", 0.2)      -- seconds

-- Custom GLSL shader (picom, requires GLX backend)
-- gar.set("picom_shader", "~/.config/gar/shaders/focused-glow.glsl")

-- Per-window picom rules (examples)
-- gar.picom_rule({
--     match = "class_g = 'Firefox'",
--     corner_radius = 8,
--     opacity = 0.95,
-- })
-- gar.picom_rule({
--     match = "class_g = 'Alacritty'",
--     blur_background = true,
--     opacity = 0.9,
-- })

-- Behavior
gar.set("follow_window_on_move", true)  -- Follow window when using Mod+Shift+number

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

-- Toggle fullscreen
gar.bind(mod .. "+shift+f", gar.toggle_fullscreen)

-- Cycle through floating windows
gar.bind(mod .. "+grave", gar.cycle_floating)  -- Mod+` (backtick)

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

-- Launchers (rofi)
-- Use -theme gar if you've installed config/rofi-gar.rasi to ~/.config/rofi/
gar.bind(mod .. "+space", function()
    gar.exec("rofi -show drun -show-icons")
end)
gar.bind(mod .. "+Tab", function()
    gar.exec("rofi -show window -show-icons")  -- Window switcher (uses EWMH)
end)
gar.bind(mod .. "+r", function()
    gar.exec("rofi -show run")
end)

-- dmenu alternative (if rofi not available)
gar.bind(mod .. "+p", function()
    gar.exec("dmenu_run")
end)

-- Screenshot (requires scrot or maim)
gar.bind("Print", function()
    gar.exec("scrot -e 'mv $f ~/Pictures/' || maim ~/Pictures/screenshot-$(date +%s).png")
end)
gar.bind(mod .. "+Print", function()
    gar.exec("scrot -s -e 'mv $f ~/Pictures/' || maim -s ~/Pictures/screenshot-$(date +%s).png")
end)

-- Lock screen (requires i3lock, swaylock, or slock)
gar.bind(mod .. "+Escape", function()
    gar.exec("i3lock -c 000000 || swaylock -c 000000 || slock")
end)

-- Volume controls (requires pactl/pamixer)
gar.bind("XF86AudioRaiseVolume", function()
    gar.exec("pactl set-sink-volume @DEFAULT_SINK@ +5% || pamixer -i 5")
end)
gar.bind("XF86AudioLowerVolume", function()
    gar.exec("pactl set-sink-volume @DEFAULT_SINK@ -5% || pamixer -d 5")
end)
gar.bind("XF86AudioMute", function()
    gar.exec("pactl set-sink-mute @DEFAULT_SINK@ toggle || pamixer -t")
end)

-- Brightness controls (requires brightnessctl or light)
gar.bind("XF86MonBrightnessUp", function()
    gar.exec("brightnessctl set +10% || light -A 10")
end)
gar.bind("XF86MonBrightnessDown", function()
    gar.exec("brightnessctl set 10%- || light -U 10")
end)
