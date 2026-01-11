use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, Result as LuaResult, Table, Value};
use x11rb::protocol::xproto::ModMask;

use super::Config;

/// Actions that can be triggered by keybinds
#[derive(Debug, Clone)]
pub enum Action {
    Exec(String),
    Focus(String),
    Swap(String),
    Resize(String, f32),
    CloseWindow,
    Workspace(usize),
    MoveToWorkspace(usize),
    Equalize,
    Reload,
    Exit,
    ToggleFloating,
    LuaCallback(usize), // Index into callback registry
}

/// A registered keybind
#[derive(Debug, Clone)]
pub struct Keybind {
    pub modifiers: ModMask,
    pub keysym: u32,
    pub action: Action,
}

/// Shared state between Lua and Rust
pub struct LuaState {
    pub config: Config,
    pub keybinds: Vec<Keybind>,
    pub callbacks: Vec<mlua::RegistryKey>,
}

impl Default for LuaState {
    fn default() -> Self {
        Self {
            config: Config::default(),
            keybinds: Vec::new(),
            callbacks: Vec::new(),
        }
    }
}

pub struct LuaConfig {
    lua: Lua,
    state: Arc<Mutex<LuaState>>,
}

impl LuaConfig {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        let state = Arc::new(Mutex::new(LuaState::default()));

        Ok(Self { lua, state })
    }

    /// Get the shared state
    pub fn state(&self) -> Arc<Mutex<LuaState>> {
        Arc::clone(&self.state)
    }

    /// Load and execute configuration
    pub fn load(&self) -> LuaResult<()> {
        // Set up the gar global table with all APIs
        self.setup_api()?;

        // Find config file
        let config_path = Self::find_config();

        let source = match config_path {
            Some(path) => {
                tracing::info!("Loading config from {:?}", path);
                std::fs::read_to_string(&path).map_err(|e| mlua::Error::external(e))?
            }
            None => {
                tracing::info!("Using default config");
                include_str!("../../config/default.lua").to_string()
            }
        };

        self.lua.load(&source).exec()?;

        let state = self.state.lock().unwrap();
        tracing::info!(
            "Config loaded: {} keybinds registered",
            state.keybinds.len()
        );

        Ok(())
    }

    /// Reload configuration (clears existing keybinds)
    pub fn reload(&self) -> LuaResult<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.keybinds.clear();
            state.callbacks.clear();
            state.config = Config::default();
        }
        self.load()
    }

    /// Execute a Lua callback by registry index
    pub fn execute_callback(&self, index: usize) -> LuaResult<()> {
        let state = self.state.lock().unwrap();
        if let Some(key) = state.callbacks.get(index) {
            let func: Function = self.lua.registry_value(key)?;
            drop(state); // Release lock before calling
            func.call::<()>(())?;
        }
        Ok(())
    }

    fn find_config() -> Option<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("gar/init.lua"))
            .filter(|p| p.exists())
    }

    fn setup_api(&self) -> LuaResult<()> {
        let gar = self.lua.create_table()?;

        // gar.set(key, value)
        self.register_set(&gar)?;

        // gar.bind(keyspec, callback)
        self.register_bind(&gar)?;

        // gar.exec(cmd)
        self.register_exec(&gar)?;

        // Built-in action functions
        self.register_actions(&gar)?;

        self.lua.globals().set("gar", gar)?;
        Ok(())
    }

    fn register_set(&self, gar: &Table) -> LuaResult<()> {
        let state = Arc::clone(&self.state);
        let set_fn = self.lua.create_function(move |_, (key, value): (String, Value)| {
            let mut state = state.lock().unwrap();
            match key.as_str() {
                "border_width" => {
                    if let Value::Integer(v) = value {
                        state.config.border_width = v as u32;
                    }
                }
                "border_color_focused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_color_focused = color;
                            }
                        }
                    }
                }
                "border_color_unfocused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_color_unfocused = color;
                            }
                        }
                    }
                }
                "gap_inner" => {
                    if let Value::Integer(v) = value {
                        state.config.gap_inner = v as u32;
                    }
                }
                "gap_outer" => {
                    if let Value::Integer(v) = value {
                        state.config.gap_outer = v as u32;
                    }
                }
                _ => {
                    tracing::warn!("Unknown config key: {}", key);
                }
            }
            Ok(())
        })?;
        gar.set("set", set_fn)
    }

    fn register_bind(&self, gar: &Table) -> LuaResult<()> {
        let state = Arc::clone(&self.state);
        let lua_weak = self.lua.clone();

        let bind_fn = self.lua.create_function(move |_, (keyspec, callback): (String, Value)| {
            let (modifiers, keysym) = match parse_keyspec(&keyspec) {
                Some(k) => k,
                None => {
                    tracing::warn!("Invalid keyspec: {}", keyspec);
                    return Ok(());
                }
            };

            let action = match callback {
                Value::Function(f) => {
                    // Store callback in registry
                    let key = lua_weak.create_registry_value(f)?;
                    let mut state = state.lock().unwrap();
                    let index = state.callbacks.len();
                    state.callbacks.push(key);
                    Action::LuaCallback(index)
                }
                Value::Table(t) => {
                    // Check if it's a built-in action table
                    if let Ok(action_type) = t.get::<String>("action") {
                        match action_type.as_str() {
                            "close_window" => Action::CloseWindow,
                            "reload" => Action::Reload,
                            "exit" => Action::Exit,
                            "equalize" => Action::Equalize,
                            "toggle_floating" => Action::ToggleFloating,
                            "focus" => {
                                let dir: String = t.get("direction").unwrap_or_default();
                                Action::Focus(dir)
                            }
                            "swap" => {
                                let dir: String = t.get("direction").unwrap_or_default();
                                Action::Swap(dir)
                            }
                            "resize" => {
                                let dir: String = t.get("direction").unwrap_or_default();
                                let amount: f32 = t.get("amount").unwrap_or(0.05);
                                Action::Resize(dir, amount)
                            }
                            "workspace" => {
                                let n: usize = t.get("workspace").unwrap_or(1);
                                Action::Workspace(n)
                            }
                            "move_to_workspace" => {
                                let n: usize = t.get("workspace").unwrap_or(1);
                                Action::MoveToWorkspace(n)
                            }
                            _ => {
                                tracing::warn!("Unknown action type: {}", action_type);
                                return Ok(());
                            }
                        }
                    } else {
                        return Ok(());
                    }
                }
                _ => return Ok(()),
            };

            let mut state = state.lock().unwrap();
            state.keybinds.push(Keybind {
                modifiers,
                keysym,
                action,
            });

            tracing::debug!("Bound {:?}+{:x} to {:?}", modifiers, keysym, state.keybinds.last().unwrap().action);
            Ok(())
        })?;
        gar.set("bind", bind_fn)
    }

    fn register_exec(&self, gar: &Table) -> LuaResult<()> {
        let exec_fn = self.lua.create_function(|_, cmd: String| {
            tracing::debug!("exec: {}", cmd);
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .spawn()
                .ok();
            Ok(())
        })?;
        gar.set("exec", exec_fn)
    }

    fn register_actions(&self, gar: &Table) -> LuaResult<()> {
        // gar.close_window - returns a table that bind() recognizes
        let close_window = self.lua.create_table()?;
        close_window.set("action", "close_window")?;
        gar.set("close_window", close_window)?;

        // gar.reload
        let reload = self.lua.create_table()?;
        reload.set("action", "reload")?;
        gar.set("reload", reload)?;

        // gar.exit
        let exit = self.lua.create_table()?;
        exit.set("action", "exit")?;
        gar.set("exit", exit)?;

        // gar.equalize
        let equalize = self.lua.create_table()?;
        equalize.set("action", "equalize")?;
        gar.set("equalize", equalize)?;

        // gar.toggle_floating
        let toggle_floating = self.lua.create_table()?;
        toggle_floating.set("action", "toggle_floating")?;
        gar.set("toggle_floating", toggle_floating)?;

        // gar.focus(direction) - creates action
        let focus_fn = self.lua.create_function(|lua, direction: String| {
            let t = lua.create_table()?;
            t.set("action", "focus")?;
            t.set("direction", direction)?;
            Ok(t)
        })?;
        gar.set("focus", focus_fn)?;

        // gar.swap(direction)
        let swap_fn = self.lua.create_function(|lua, direction: String| {
            let t = lua.create_table()?;
            t.set("action", "swap")?;
            t.set("direction", direction)?;
            Ok(t)
        })?;
        gar.set("swap", swap_fn)?;

        // gar.resize(direction, amount)
        let resize_fn = self.lua.create_function(|lua, (direction, amount): (String, f32)| {
            let t = lua.create_table()?;
            t.set("action", "resize")?;
            t.set("direction", direction)?;
            t.set("amount", amount)?;
            Ok(t)
        })?;
        gar.set("resize", resize_fn)?;

        // gar.workspace(n)
        let workspace_fn = self.lua.create_function(|lua, n: usize| {
            let t = lua.create_table()?;
            t.set("action", "workspace")?;
            t.set("workspace", n)?;
            Ok(t)
        })?;
        gar.set("workspace", workspace_fn)?;

        // gar.move_to_workspace(n)
        let move_fn = self.lua.create_function(|lua, n: usize| {
            let t = lua.create_table()?;
            t.set("action", "move_to_workspace")?;
            t.set("workspace", n)?;
            Ok(t)
        })?;
        gar.set("move_to_workspace", move_fn)?;

        Ok(())
    }
}

/// Parse a hex color string like "#5294e2" to u32
fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim_start_matches('#');
    u32::from_str_radix(s, 16).ok()
}

/// Parse a keyspec like "mod+shift+q" into (ModMask, keysym)
fn parse_keyspec(spec: &str) -> Option<(ModMask, u32)> {
    let lowercase = spec.to_lowercase();
    let parts: Vec<&str> = lowercase.split('+').collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = ModMask::from(0u16);
    let mut key_part = None;

    for part in &parts {
        match *part {
            "mod" | "super" | "mod4" => modifiers |= ModMask::M4,
            "alt" | "mod1" => modifiers |= ModMask::M1,
            "shift" => modifiers |= ModMask::SHIFT,
            "ctrl" | "control" => modifiers |= ModMask::CONTROL,
            _ => key_part = Some(*part),
        }
    }

    let key = key_part?;
    let keysym = match key {
        "return" | "enter" => 0xff0d,
        "escape" | "esc" => 0xff1b,
        "tab" => 0xff09,
        "space" => 0x20,
        "backspace" => 0xff08,
        "delete" => 0xffff,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "home" => 0xff50,
        "end" => 0xff57,
        "page_up" | "pageup" => 0xff55,
        "page_down" | "pagedown" => 0xff56,
        "f1" => 0xffbe,
        "f2" => 0xffbf,
        "f3" => 0xffc0,
        "f4" => 0xffc1,
        "f5" => 0xffc2,
        "f6" => 0xffc3,
        "f7" => 0xffc4,
        "f8" => 0xffc5,
        "f9" => 0xffc6,
        "f10" => 0xffc7,
        "f11" => 0xffc8,
        "f12" => 0xffc9,
        // Numbers
        "0" => 0x30,
        "1" => 0x31,
        "2" => 0x32,
        "3" => 0x33,
        "4" => 0x34,
        "5" => 0x35,
        "6" => 0x36,
        "7" => 0x37,
        "8" => 0x38,
        "9" => 0x39,
        // Letters (lowercase keysyms)
        s if s.len() == 1 => {
            let c = s.chars().next()?;
            if c.is_ascii_lowercase() {
                c as u32
            } else {
                return None;
            }
        }
        _ => return None,
    };

    Some((modifiers, keysym))
}
