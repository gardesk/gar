mod lua;

pub use lua::{Action, Keybind, LuaConfig, LuaState, RuleActions, WindowMatch, WindowRule};

#[derive(Debug, Clone)]
pub struct Config {
    pub border_width: u32,
    pub border_color_focused: u32,
    pub border_color_unfocused: u32,
    pub border_color_urgent: u32,
    pub gap_inner: u32,
    pub gap_outer: u32,
    // Title bar settings
    pub titlebar_enabled: bool,
    pub titlebar_height: u32,
    pub titlebar_color_focused: u32,
    pub titlebar_color_unfocused: u32,
    pub titlebar_text_color: u32,
    // Behavior settings
    pub follow_window_on_move: bool,
    pub mouse_follows_focus: bool,
    // Manual bar/panel reserved space (overrides struts)
    pub bar_height: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            border_width: 2,
            border_color_focused: 0x5294e2,
            border_color_unfocused: 0x2d2d2d,
            border_color_urgent: 0xff5555, // Red for urgent windows
            gap_inner: 0,
            gap_outer: 0,
            // Title bars disabled by default
            titlebar_enabled: false,
            titlebar_height: 20,
            titlebar_color_focused: 0x3d3d3d,
            titlebar_color_unfocused: 0x2d2d2d,
            titlebar_text_color: 0xffffff,
            // Behavior: follow window when moving to another workspace
            follow_window_on_move: false,
            // Behavior: warp mouse pointer to center of focused window
            mouse_follows_focus: false,
            // Manual bar height (0 = use struts from dock windows)
            bar_height: 0,
        }
    }
}
