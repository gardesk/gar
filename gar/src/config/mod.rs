mod lua;

pub use lua::{Action, Keybind, LuaConfig, LuaState, RuleActions, WindowMatch, WindowRule};

/// A per-window picom rule for customizing compositor effects per application
#[derive(Debug, Clone, Default)]
pub struct PicomRule {
    pub match_expr: String,
    pub corner_radius: Option<u32>,
    pub opacity: Option<f64>,
    pub shadow: Option<bool>,
    pub blur_background: Option<bool>,
    pub shader: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub border_width: u32,
    pub border_color_focused: u32,
    pub border_color_unfocused: u32,
    pub border_color_urgent: u32,
    pub border_color_swap_target: u32,
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
    // garbar integration: spawn garbar automatically if gar.bar is configured
    pub bar_enabled: bool,
    // garnotify integration: spawn garnotify automatically if gar.notification is configured
    pub notification_enabled: bool,
    // Monitor ordering: list of monitor names in desired left-to-right order
    // If empty, monitors are sorted by X position (default)
    pub monitor_order: Vec<String>,
    // Screen timeout/DPMS settings
    pub screen_timeout_enabled: bool,
    pub screen_timeout_seconds: u32,
    // Compositor visual settings (picom)
    // These are stored for reference and potential dynamic picom config generation
    pub corner_radius: u32,
    pub blur_enabled: bool,
    pub blur_method: String,
    pub blur_strength: u32,
    pub shadow_enabled: bool,
    pub shadow_radius: u32,
    pub shadow_opacity: f64,
    pub shadow_offset_x: i32,
    pub shadow_offset_y: i32,
    pub opacity_focused: f64,
    pub opacity_unfocused: f64,
    pub fade_enabled: bool,
    pub fade_delta: u32,
    // Border gradient settings
    pub border_gradient_enabled: bool,
    pub border_gradient_start_focused: u32,
    pub border_gradient_end_focused: u32,
    pub border_gradient_start_unfocused: u32,
    pub border_gradient_end_unfocused: u32,
    pub border_gradient_direction: String,
    // Animation settings
    pub animation_open: String,
    pub animation_close: String,
    pub animation_duration: f64,
    pub animation_curve: String,
    // Shader and per-window rules
    pub picom_shader: Option<String>,
    pub picom_rules: Vec<PicomRule>,
}

impl Config {
    /// Generate picom.conf content from current config settings.
    pub fn generate_picom_config(&self) -> String {
        let blur_section = if self.blur_enabled {
            format!(
                r#"# Blur
blur-method = "{}";
blur-strength = {};
blur-background = true;
blur-background-frame = false;
blur-kern = "3x3box";

blur-background-exclude = [
    "window_type = 'dock'",
    "window_type = 'desktop'",
    "window_type = 'menu'",
    "window_type = 'dropdown_menu'",
    "window_type = 'popup_menu'",
    "_NET_WM_BYPASS_COMPOSITOR = 1"
];"#,
                self.blur_method, self.blur_strength
            )
        } else {
            "# Blur disabled".to_string()
        };

        let shadow_section = if self.shadow_enabled {
            format!(
                r#"# Shadows
shadow = true;
shadow-radius = {};
shadow-opacity = {:.2};
shadow-offset-x = {};
shadow-offset-y = {};

shadow-exclude = [
    "window_type = 'dock'",
    "window_type = 'desktop'",
    "window_type = 'menu'",
    "window_type = 'dropdown_menu'",
    "window_type = 'popup_menu'",
    "window_type = 'tooltip'",
    "_NET_WM_STATE *= '_NET_WM_STATE_FULLSCREEN'",
    "_NET_WM_BYPASS_COMPOSITOR = 1"
];"#,
                self.shadow_radius,
                self.shadow_opacity,
                self.shadow_offset_x,
                self.shadow_offset_y
            )
        } else {
            "# Shadows disabled\nshadow = false;".to_string()
        };

        let fade_section = if self.fade_enabled {
            format!(
                r#"# Fading / Animations
fading = true;
fade-in-step = 0.028;
fade-out-step = 0.03;
fade-delta = {};

no-fading-destroyed-argb = true;

fade-exclude = [
    "window_type = 'menu'",
    "window_type = 'dropdown_menu'",
    "window_type = 'popup_menu'"
];"#,
                self.fade_delta
            )
        } else {
            "# Fading disabled\nfading = false;".to_string()
        };

        let opacity_section = if self.opacity_unfocused < 1.0 {
            format!(
                r#"# Focus Opacity
active-opacity = {:.2};
inactive-opacity = {:.2};
frame-opacity = 1.0;"#,
                self.opacity_focused, self.opacity_unfocused
            )
        } else {
            "# Focus opacity: all windows fully opaque".to_string()
        };

        // Animation section - only generate if using valid picom v12 presets
        // Valid presets: slide-in, slide-out, fly-in, fly-out, appear, disappear
        let valid_presets = ["slide-in", "slide-out", "fly-in", "fly-out", "appear", "disappear"];
        let open_valid = valid_presets.contains(&self.animation_open.as_str());
        let close_valid = valid_presets.contains(&self.animation_close.as_str());

        let animation_section = if open_valid || close_valid {
            let open_preset = if open_valid { &self.animation_open } else { "appear" };
            let close_preset = if close_valid { &self.animation_close } else { "disappear" };
            format!(
                r#"# Animations
animations = ({{
    triggers = ["open", "show"];
    preset = "{}";
    duration = {:.2};
}},
{{
    triggers = ["close", "hide"];
    preset = "{}";
    duration = {:.2};
}},
{{
    triggers = ["geometry"];
    preset = "geometry-change";
    duration = {:.2};
}});"#,
                open_preset, self.animation_duration,
                close_preset, self.animation_duration,
                self.animation_duration * 0.5
            )
        } else {
            "# Animations disabled".to_string()
        };

        // Global shader section
        let shader_section = if let Some(ref shader) = self.picom_shader {
            // Expand ~ to home directory
            let expanded = if shader.starts_with("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(&shader[2..]).to_string_lossy().to_string()
                } else {
                    shader.clone()
                }
            } else {
                shader.clone()
            };
            format!("# Custom Shader\nwindow-shader-fg = \"{}\";", expanded)
        } else {
            "# No custom shader".to_string()
        };

        // Per-window rules section
        let rules_section = if !self.picom_rules.is_empty() {
            let mut rules = String::from("# Per-window Rules\nrules = (\n");
            for rule in &self.picom_rules {
                rules.push_str(&format!("    {{\n        match = \"{}\";\n", rule.match_expr));
                if let Some(cr) = rule.corner_radius {
                    rules.push_str(&format!("        corner-radius = {};\n", cr));
                }
                if let Some(opacity) = rule.opacity {
                    rules.push_str(&format!("        opacity = {:.2};\n", opacity));
                }
                if let Some(shadow) = rule.shadow {
                    rules.push_str(&format!("        shadow = {};\n", shadow));
                }
                if let Some(blur) = rule.blur_background {
                    rules.push_str(&format!("        blur-background = {};\n", blur));
                }
                if let Some(ref shader) = rule.shader {
                    let expanded = if shader.starts_with("~/") {
                        if let Some(home) = dirs::home_dir() {
                            home.join(&shader[2..]).to_string_lossy().to_string()
                        } else {
                            shader.clone()
                        }
                    } else {
                        shader.clone()
                    };
                    rules.push_str(&format!("        shader = \"{}\";\n", expanded));
                }
                rules.push_str("    },\n");
            }
            rules.push_str(");");
            rules
        } else {
            "# No per-window rules".to_string()
        };

        format!(
            r#"# picom.conf - Auto-generated by gar window manager
# DO NOT EDIT MANUALLY - changes will be overwritten on reload
# Edit ~/.config/gar/init.lua instead and reload with Mod+Shift+R

# Backend Configuration
backend = "glx";
vsync = true;
use-ewmh-active-win = true;
glx-no-stencil = true;

# Rounded Corners
corner-radius = {};

rounded-corners-exclude = [
    "window_type = 'dock'",
    "window_type = 'desktop'",
    "window_type = 'tooltip'",
    "window_type = 'menu'",
    "window_type = 'dropdown_menu'",
    "window_type = 'popup_menu'",
    "_NET_WM_STATE *= '_NET_WM_STATE_FULLSCREEN'"
];

{}

{}

{}

{}

{}

{}

{}

# Window Type Settings
wintypes:
{{
    tooltip = {{
        fade = true;
        shadow = false;
        opacity = 0.95;
        focus = true;
        blur-background = false;
    }};
    dock = {{
        shadow = false;
        clip-shadow-above = true;
    }};
    dnd = {{
        shadow = false;
    }};
    popup_menu = {{
        opacity = 0.95;
        shadow = false;
    }};
    dropdown_menu = {{
        opacity = 0.95;
        shadow = false;
    }};
}};
"#,
            self.corner_radius,
            blur_section,
            shadow_section,
            fade_section,
            opacity_section,
            animation_section,
            shader_section,
            rules_section
        )
    }

    /// Write picom config to ~/.config/gar/picom.conf and signal picom to reload.
    pub fn write_picom_config(&self) -> std::io::Result<()> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory"
            ))?
            .join("gar");

        // Ensure directory exists
        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join("picom.conf");
        let content = self.generate_picom_config();

        std::fs::write(&config_path, &content)?;
        tracing::info!("Generated picom config at {:?}", config_path);

        // Signal picom to reload
        Self::reload_picom();

        Ok(())
    }

    /// Apply screen timeout/DPMS settings using xset
    pub fn apply_screen_timeout(&self) {
        use std::process::Command;

        if self.screen_timeout_enabled {
            // Enable DPMS and set timeout
            let timeout = self.screen_timeout_seconds.to_string();
            match Command::new("xset")
                .args(["dpms", &timeout, &timeout, &timeout])
                .status()
            {
                Ok(status) if status.success() => {
                    tracing::info!("Set DPMS timeout to {} seconds", self.screen_timeout_seconds);
                }
                Ok(_) => tracing::warn!("xset dpms command failed"),
                Err(e) => tracing::warn!("Failed to run xset: {}", e),
            }

            // Enable screen saver with same timeout
            match Command::new("xset")
                .args(["s", &timeout, &timeout])
                .status()
            {
                Ok(status) if status.success() => {
                    tracing::debug!("Set screen saver timeout to {} seconds", self.screen_timeout_seconds);
                }
                Ok(_) => tracing::warn!("xset s command failed"),
                Err(e) => tracing::warn!("Failed to run xset: {}", e),
            }

            // Make sure DPMS is enabled
            match Command::new("xset").args(["+dpms"]).status() {
                Ok(_) => {}
                Err(e) => tracing::warn!("Failed to enable DPMS: {}", e),
            }
        } else {
            // Disable DPMS and screen saver
            match Command::new("xset").args(["dpms", "0", "0", "0"]).status() {
                Ok(status) if status.success() => {
                    tracing::info!("Disabled DPMS timeout");
                }
                Ok(_) => tracing::warn!("xset dpms 0 command failed"),
                Err(e) => tracing::warn!("Failed to run xset: {}", e),
            }

            match Command::new("xset").args(["s", "off"]).status() {
                Ok(status) if status.success() => {
                    tracing::debug!("Disabled screen saver");
                }
                Ok(_) => tracing::warn!("xset s off command failed"),
                Err(e) => tracing::warn!("Failed to run xset: {}", e),
            }

            match Command::new("xset").args(["-dpms"]).status() {
                Ok(_) => {}
                Err(e) => tracing::warn!("Failed to disable DPMS: {}", e),
            }
        }
    }

    /// Restart picom to apply new configuration.
    /// Picom doesn't support config reload via signal, so we kill and restart it.
    fn reload_picom() {
        use std::process::Command;
        use std::thread;
        use std::time::Duration;

        // Kill existing picom
        match Command::new("pkill").arg("picom").status() {
            Ok(status) if status.success() => {
                tracing::info!("Killed picom for restart");
            }
            Ok(_) => {
                tracing::debug!("picom was not running");
            }
            Err(e) => {
                tracing::warn!("Failed to kill picom: {}", e);
                return;
            }
        }

        // Brief pause to let picom fully exit
        thread::sleep(Duration::from_millis(100));

        // Restart picom with the new config
        let config_path = dirs::config_dir()
            .map(|d| d.join("gar").join("picom.conf"))
            .unwrap_or_default();

        match Command::new("picom")
            .args(["-b", "--config"])
            .arg(&config_path)
            .spawn()
        {
            Ok(_) => {
                tracing::info!("Restarted picom with config {:?}", config_path);
            }
            Err(e) => {
                tracing::warn!("Failed to restart picom: {}", e);
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            border_width: 2,
            border_color_focused: 0x5294e2,
            border_color_unfocused: 0x2d2d2d,
            border_color_urgent: 0xff5555, // Red for urgent windows
            border_color_swap_target: 0x00ff00, // Green for drag-swap target
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
            // garbar not enabled by default (enabled when gar.bar table is set)
            bar_enabled: false,
            // garnotify not enabled by default (enabled when gar.notification table is set)
            notification_enabled: false,
            // Monitor order: empty = sort by X position
            monitor_order: Vec::new(),
            // Screen timeout: enabled by default with 10 minute timeout
            screen_timeout_enabled: true,
            screen_timeout_seconds: 600,
            // Compositor settings (picom) - matching picom.conf defaults
            corner_radius: 12,
            blur_enabled: true,
            blur_method: "dual_kawase".to_string(),
            blur_strength: 5,
            shadow_enabled: true,
            shadow_radius: 12,
            shadow_opacity: 0.75,
            shadow_offset_x: -7,
            shadow_offset_y: -7,
            opacity_focused: 1.0,
            opacity_unfocused: 1.0, // No unfocused dimming by default
            fade_enabled: true,
            fade_delta: 10,
            // Border gradients disabled by default
            border_gradient_enabled: false,
            border_gradient_start_focused: 0x5294e2,
            border_gradient_end_focused: 0x1a5fb4,
            border_gradient_start_unfocused: 0x3d3d3d,
            border_gradient_end_unfocused: 0x1d1d1d,
            border_gradient_direction: "vertical".to_string(),
            // Animations disabled by default
            animation_open: "none".to_string(),
            animation_close: "none".to_string(),
            animation_duration: 0.2,
            animation_curve: "ease-out".to_string(),
            // No global shader by default
            picom_shader: None,
            picom_rules: Vec::new(),
        }
    }
}
