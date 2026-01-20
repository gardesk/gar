use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, Result as LuaResult, Table, Value};
use x11rb::protocol::xproto::ModMask;

use super::{Config, PicomRule};

/// Actions that can be triggered by keybinds
#[derive(Debug, Clone)]
pub enum Action {
    Exec(String),
    Focus(String),
    Swap(String),
    Resize(String, f32),
    CloseWindow,
    ForceCloseWindow,  // Force kill without asking nicely
    Workspace(usize),
    WorkspaceNext,
    WorkspacePrev,
    MoveToWorkspace(usize),
    Equalize,
    Reload,
    Exit,
    ToggleFloating,
    ToggleFullscreen,
    CycleFloating,
    FocusMonitor(String),     // "next", "prev", or monitor name
    MoveToMonitor(String),    // "next", "prev", or monitor name
    LuaCallback(usize), // Index into callback registry
}

/// A registered keybind
#[derive(Debug, Clone)]
pub struct Keybind {
    pub modifiers: ModMask,
    pub keysym: u32,
    pub action: Action,
}

/// Window rule matching criteria
#[derive(Debug, Clone, Default)]
pub struct WindowMatch {
    pub class: Option<String>,
    pub instance: Option<String>,
    pub title: Option<String>,
}

/// Actions to apply when a rule matches
#[derive(Debug, Clone, Default)]
pub struct RuleActions {
    pub floating: Option<bool>,
    pub workspace: Option<usize>,
}

/// A window rule: if match criteria are met, apply actions
#[derive(Debug, Clone)]
pub struct WindowRule {
    pub match_criteria: WindowMatch,
    pub actions: RuleActions,
}

/// Shared state between Lua and Rust
pub struct LuaState {
    pub config: Config,
    pub keybinds: Vec<Keybind>,
    pub callbacks: Vec<mlua::RegistryKey>,
    pub rules: Vec<WindowRule>,
    pub exec_once_cmds: HashSet<String>,
}

impl Default for LuaState {
    fn default() -> Self {
        Self {
            config: Config::default(),
            keybinds: Vec::new(),
            callbacks: Vec::new(),
            rules: Vec::new(),
            exec_once_cmds: HashSet::new(),
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

        // Find or create config file
        let config_path = Self::find_or_create_config()?;

        tracing::info!("Loading config from {:?}", config_path);
        let source = std::fs::read_to_string(&config_path)
            .map_err(|e| mlua::Error::external(e))?;

        self.lua.load(&source).exec()?;

        // Check if gar.bar table exists - enables garbar integration
        self.check_bar_config()?;

        let state = self.state.lock().unwrap();
        tracing::info!(
            "Config loaded: {} keybinds registered, bar_enabled={}",
            state.keybinds.len(),
            state.config.bar_enabled
        );

        Ok(())
    }

    /// Check if gar.bar table is configured, enabling garbar integration
    fn check_bar_config(&self) -> LuaResult<()> {
        let globals = self.lua.globals();
        let gar: Table = globals.get("gar")?;

        // Check if gar.bar exists and is a table
        match gar.get::<Table>("bar") {
            Ok(bar_table) => {
                // gar.bar exists! Enable garbar integration
                let mut state = self.state.lock().unwrap();
                state.config.bar_enabled = true;

                // Optionally read bar height from config for reserved space
                if let Ok(height) = bar_table.get::<u32>("height") {
                    state.config.bar_height = height;
                    tracing::info!("garbar integration enabled (height={})", height);
                } else {
                    tracing::info!("garbar integration enabled (default height)");
                }
            }
            Err(_) => {
                // gar.bar not set, garbar won't be spawned
                tracing::debug!("gar.bar not configured, garbar integration disabled");
            }
        }

        Ok(())
    }

    /// Reload configuration (clears existing keybinds and rules)
    pub fn reload(&self) -> LuaResult<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.keybinds.clear();
            state.callbacks.clear();
            state.rules.clear();
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

    /// Find user config or create it from defaults if it doesn't exist.
    fn find_or_create_config() -> LuaResult<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| mlua::Error::external("Could not determine config directory"))?
            .join("gar");
        let config_path = config_dir.join("init.lua");

        if config_path.exists() {
            return Ok(config_path);
        }

        // Create config directory if needed
        if !config_dir.exists() {
            std::fs::create_dir_all(&config_dir)
                .map_err(|e| mlua::Error::external(format!("Failed to create config dir: {}", e)))?;
            tracing::info!("Created config directory: {:?}", config_dir);
        }

        // Write default config
        let default_config = include_str!("../../config/default.lua");
        std::fs::write(&config_path, default_config)
            .map_err(|e| mlua::Error::external(format!("Failed to write default config: {}", e)))?;
        tracing::info!("Created default config at {:?}", config_path);

        Ok(config_path)
    }

    fn setup_api(&self) -> LuaResult<()> {
        let gar = self.lua.create_table()?;

        // gar.set(key, value)
        self.register_set(&gar)?;

        // gar.bind(keyspec, callback)
        self.register_bind(&gar)?;

        // gar.exec(cmd)
        self.register_exec(&gar)?;

        // gar.rule(match, actions)
        self.register_rule(&gar)?;

        // gar.picom_rule(table) - per-window picom rules
        self.register_picom_rule(&gar)?;

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
                "border_color_urgent" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_color_urgent = color;
                            }
                        }
                    }
                }
                "border_color_swap_target" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_color_swap_target = color;
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
                "titlebar_enabled" => {
                    if let Value::Boolean(v) = value {
                        state.config.titlebar_enabled = v;
                    }
                }
                "titlebar_height" => {
                    if let Value::Integer(v) = value {
                        state.config.titlebar_height = v as u32;
                    }
                }
                "titlebar_color_focused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.titlebar_color_focused = color;
                            }
                        }
                    }
                }
                "titlebar_color_unfocused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.titlebar_color_unfocused = color;
                            }
                        }
                    }
                }
                "titlebar_text_color" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.titlebar_text_color = color;
                            }
                        }
                    }
                }
                "follow_window_on_move" => {
                    if let Value::Boolean(v) = value {
                        state.config.follow_window_on_move = v;
                    }
                }
                "mouse_follows_focus" => {
                    if let Value::Boolean(v) = value {
                        state.config.mouse_follows_focus = v;
                    }
                }
                "bar_height" => {
                    if let Value::Integer(v) = value {
                        state.config.bar_height = v as u32;
                    }
                }
                // Compositor visual settings (picom)
                "corner_radius" => {
                    if let Value::Integer(v) = value {
                        state.config.corner_radius = v as u32;
                    }
                }
                "blur_enabled" => {
                    if let Value::Boolean(v) = value {
                        state.config.blur_enabled = v;
                    }
                }
                "blur_method" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            state.config.blur_method = str_val.to_string();
                        }
                    }
                }
                "blur_strength" => {
                    if let Value::Integer(v) = value {
                        state.config.blur_strength = v as u32;
                    }
                }
                "shadow_enabled" => {
                    if let Value::Boolean(v) = value {
                        state.config.shadow_enabled = v;
                    }
                }
                "shadow_radius" => {
                    if let Value::Integer(v) = value {
                        state.config.shadow_radius = v as u32;
                    }
                }
                "shadow_opacity" => {
                    if let Value::Number(v) = value {
                        state.config.shadow_opacity = v;
                    }
                }
                "shadow_offset_x" => {
                    if let Value::Integer(v) = value {
                        state.config.shadow_offset_x = v as i32;
                    }
                }
                "shadow_offset_y" => {
                    if let Value::Integer(v) = value {
                        state.config.shadow_offset_y = v as i32;
                    }
                }
                "opacity_focused" => {
                    if let Value::Number(v) = value {
                        state.config.opacity_focused = v;
                    }
                }
                "opacity_unfocused" => {
                    if let Value::Number(v) = value {
                        state.config.opacity_unfocused = v;
                    }
                }
                "fade_enabled" => {
                    if let Value::Boolean(v) = value {
                        state.config.fade_enabled = v;
                    }
                }
                "fade_delta" => {
                    if let Value::Integer(v) = value {
                        state.config.fade_delta = v as u32;
                    }
                }
                // Border gradient settings
                "border_gradient_enabled" => {
                    if let Value::Boolean(v) = value {
                        state.config.border_gradient_enabled = v;
                    }
                }
                "border_gradient_start_focused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_gradient_start_focused = color;
                            }
                        }
                    }
                }
                "border_gradient_end_focused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_gradient_end_focused = color;
                            }
                        }
                    }
                }
                "border_gradient_start_unfocused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_gradient_start_unfocused = color;
                            }
                        }
                    }
                }
                "border_gradient_end_unfocused" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            if let Some(color) = parse_color(&str_val) {
                                state.config.border_gradient_end_unfocused = color;
                            }
                        }
                    }
                }
                "border_gradient_direction" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            state.config.border_gradient_direction = str_val.to_string();
                        }
                    }
                }
                // Animation settings
                "animation_open" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            state.config.animation_open = str_val.to_string();
                        }
                    }
                }
                "animation_close" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            state.config.animation_close = str_val.to_string();
                        }
                    }
                }
                "animation_duration" => {
                    if let Value::Number(v) = value {
                        state.config.animation_duration = v;
                    }
                }
                "animation_curve" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            state.config.animation_curve = str_val.to_string();
                        }
                    }
                }
                // Shader settings
                "picom_shader" => {
                    if let Value::String(s) = value {
                        if let Ok(str_val) = s.to_str() {
                            state.config.picom_shader = Some(str_val.to_string());
                        }
                    }
                }
                // Monitor ordering: list of monitor names in left-to-right order
                "monitor_order" => {
                    if let Value::Table(t) = value {
                        let mut order = Vec::new();
                        for pair in t.pairs::<i64, String>() {
                            if let Ok((_, name)) = pair {
                                order.push(name);
                            }
                        }
                        if !order.is_empty() {
                            tracing::info!("Monitor order configured: {:?}", order);
                            state.config.monitor_order = order;
                        }
                    }
                }
                // Screen timeout settings
                "screen_timeout_enabled" => {
                    if let Value::Boolean(v) = value {
                        state.config.screen_timeout_enabled = v;
                        tracing::info!("Screen timeout enabled: {}", v);
                    }
                }
                "screen_timeout" => {
                    if let Value::Integer(v) = value {
                        state.config.screen_timeout_seconds = v as u32;
                        tracing::info!("Screen timeout set to {} seconds", v);
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
                            "force_close_window" => Action::ForceCloseWindow,
                            "reload" => Action::Reload,
                            "exit" => Action::Exit,
                            "equalize" => Action::Equalize,
                            "toggle_floating" => Action::ToggleFloating,
                            "toggle_fullscreen" => Action::ToggleFullscreen,
                            "cycle_floating" => Action::CycleFloating,
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
                            "workspace_next" => Action::WorkspaceNext,
                            "workspace_prev" => Action::WorkspacePrev,
                            "move_to_workspace" => {
                                let n: usize = t.get("workspace").unwrap_or(1);
                                Action::MoveToWorkspace(n)
                            }
                            "focus_monitor" => {
                                let target: String = t.get("target").unwrap_or_default();
                                Action::FocusMonitor(target)
                            }
                            "move_to_monitor" => {
                                let target: String = t.get("target").unwrap_or_default();
                                Action::MoveToMonitor(target)
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
        gar.set("exec", exec_fn)?;

        // gar.exec_once(cmd) - only run if not already run this session
        let state = Arc::clone(&self.state);
        let exec_once_fn = self.lua.create_function(move |_, cmd: String| {
            let mut state = state.lock().unwrap();
            if state.exec_once_cmds.contains(&cmd) {
                tracing::debug!("exec_once: skipping already-run command: {}", cmd);
                return Ok(());
            }
            tracing::info!("exec_once: {}", cmd);
            state.exec_once_cmds.insert(cmd.clone());
            drop(state); // Release lock before spawning
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .spawn()
                .ok();
            Ok(())
        })?;
        gar.set("exec_once", exec_once_fn)
    }

    fn register_rule(&self, gar: &Table) -> LuaResult<()> {
        let state = Arc::clone(&self.state);
        // gar.rule({ class = "Firefox" }, { floating = true, workspace = 2 })
        let rule_fn = self.lua.create_function(move |_, (match_table, actions_table): (Table, Table)| {
            let mut match_criteria = WindowMatch::default();
            let mut actions = RuleActions::default();

            // Parse match criteria
            if let Ok(class) = match_table.get::<String>("class") {
                match_criteria.class = Some(class);
            }
            if let Ok(instance) = match_table.get::<String>("instance") {
                match_criteria.instance = Some(instance);
            }
            if let Ok(title) = match_table.get::<String>("title") {
                match_criteria.title = Some(title);
            }

            // Parse actions
            if let Ok(floating) = actions_table.get::<bool>("floating") {
                actions.floating = Some(floating);
            }
            if let Ok(workspace) = actions_table.get::<usize>("workspace") {
                actions.workspace = Some(workspace);
            }

            let rule = WindowRule { match_criteria, actions };
            tracing::debug!("Registered rule: {:?}", rule);

            let mut state = state.lock().unwrap();
            state.rules.push(rule);
            Ok(())
        })?;
        gar.set("rule", rule_fn)
    }

    fn register_picom_rule(&self, gar: &Table) -> LuaResult<()> {
        let state = Arc::clone(&self.state);
        // gar.picom_rule({ match = "...", corner_radius = 8, opacity = 0.9, ... })
        let picom_rule_fn = self.lua.create_function(move |_, table: Table| {
            let mut rule = PicomRule::default();

            // Required: match expression
            if let Ok(match_expr) = table.get::<String>("match") {
                rule.match_expr = match_expr;
            } else {
                tracing::warn!("picom_rule: missing 'match' field");
                return Ok(());
            }

            // Optional: corner_radius
            if let Ok(cr) = table.get::<u32>("corner_radius") {
                rule.corner_radius = Some(cr);
            }

            // Optional: opacity
            if let Ok(op) = table.get::<f64>("opacity") {
                rule.opacity = Some(op);
            }

            // Optional: shadow
            if let Ok(shadow) = table.get::<bool>("shadow") {
                rule.shadow = Some(shadow);
            }

            // Optional: blur_background
            if let Ok(blur) = table.get::<bool>("blur_background") {
                rule.blur_background = Some(blur);
            }

            // Optional: shader
            if let Ok(shader) = table.get::<String>("shader") {
                rule.shader = Some(shader);
            }

            tracing::debug!("Registered picom rule: {:?}", rule);
            let mut state = state.lock().unwrap();
            state.config.picom_rules.push(rule);
            Ok(())
        })?;
        gar.set("picom_rule", picom_rule_fn)
    }

    fn register_actions(&self, gar: &Table) -> LuaResult<()> {
        // gar.close_window - returns a table that bind() recognizes
        let close_window = self.lua.create_table()?;
        close_window.set("action", "close_window")?;
        gar.set("close_window", close_window)?;

        // gar.force_close_window - force kill without asking nicely
        let force_close = self.lua.create_table()?;
        force_close.set("action", "force_close_window")?;
        gar.set("force_close_window", force_close)?;

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

        // gar.toggle_fullscreen
        let toggle_fullscreen = self.lua.create_table()?;
        toggle_fullscreen.set("action", "toggle_fullscreen")?;
        gar.set("toggle_fullscreen", toggle_fullscreen)?;

        // gar.cycle_floating
        let cycle_floating = self.lua.create_table()?;
        cycle_floating.set("action", "cycle_floating")?;
        gar.set("cycle_floating", cycle_floating)?;

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

        // gar.workspace_next()
        let workspace_next_fn = self.lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            t.set("action", "workspace_next")?;
            Ok(t)
        })?;
        gar.set("workspace_next", workspace_next_fn)?;

        // gar.workspace_prev()
        let workspace_prev_fn = self.lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            t.set("action", "workspace_prev")?;
            Ok(t)
        })?;
        gar.set("workspace_prev", workspace_prev_fn)?;

        // gar.move_to_workspace(n)
        let move_fn = self.lua.create_function(|lua, n: usize| {
            let t = lua.create_table()?;
            t.set("action", "move_to_workspace")?;
            t.set("workspace", n)?;
            Ok(t)
        })?;
        gar.set("move_to_workspace", move_fn)?;

        // gar.focus_monitor(target) - "next", "prev", or monitor name
        let focus_monitor_fn = self.lua.create_function(|lua, target: String| {
            let t = lua.create_table()?;
            t.set("action", "focus_monitor")?;
            t.set("target", target)?;
            Ok(t)
        })?;
        gar.set("focus_monitor", focus_monitor_fn)?;

        // gar.move_to_monitor(target) - "next", "prev", or monitor name
        let move_to_monitor_fn = self.lua.create_function(|lua, target: String| {
            let t = lua.create_table()?;
            t.set("action", "move_to_monitor")?;
            t.set("target", target)?;
            Ok(t)
        })?;
        gar.set("move_to_monitor", move_to_monitor_fn)?;

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
        // Punctuation
        "comma" => 0x2c,
        "period" => 0x2e,
        "semicolon" => 0x3b,
        "apostrophe" => 0x27,
        "bracketleft" => 0x5b,
        "bracketright" => 0x5d,
        "backslash" => 0x5c,
        "slash" => 0x2f,
        "minus" => 0x2d,
        "equal" => 0x3d,
        "grave" => 0x60,
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
