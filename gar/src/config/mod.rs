mod lua;

pub use lua::{Action, Keybind, LuaConfig, LuaState};

#[derive(Debug, Clone)]
pub struct Config {
    pub border_width: u32,
    pub border_color_focused: u32,
    pub border_color_unfocused: u32,
    pub gap_inner: u32,
    pub gap_outer: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            border_width: 2,
            border_color_focused: 0x5294e2,
            border_color_unfocused: 0x2d2d2d,
            gap_inner: 0,
            gap_outer: 0,
        }
    }
}
