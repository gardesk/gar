use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "garctl")]
#[command(about = "Control the gar window manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Focus window in direction
    Focus {
        /// Direction: left, right, up, down
        direction: String,
    },
    /// Swap with window in direction
    Swap {
        /// Direction: left, right, up, down
        direction: String,
    },
    /// Resize split in direction
    Resize {
        /// Direction: left, right, up, down
        direction: String,
        /// Resize amount (default: 0.05)
        #[arg(default_value = "0.05")]
        amount: f32,
    },
    /// Close focused window
    Close,
    /// Switch to workspace
    Workspace {
        /// Workspace number (1-10)
        number: usize,
    },
    /// Move focused window to workspace
    MoveToWorkspace {
        /// Workspace number (1-10)
        number: usize,
    },
    /// Toggle floating state of focused window
    ToggleFloating,
    /// Equalize split ratios
    Equalize,
    /// Reload configuration
    Reload,
    /// Exit gar
    Exit,
    /// Get workspace information
    GetWorkspaces,
    /// Get focused window information
    GetFocused,
    /// Get window tree
    GetTree,
    /// Focus monitor (next, prev, or name)
    FocusMonitor {
        /// Target: next, prev, left, right, or monitor name
        target: String,
    },
    /// Move focused window to monitor
    MoveToMonitor {
        /// Target: next, prev, left, right, or monitor name
        target: String,
    },
    /// Get monitor information
    GetMonitors,
}

fn get_socket_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("gar.sock"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/gar.sock"))
}

fn send_command(request: Value) -> Result<Value, String> {
    let socket_path = get_socket_path();

    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| format!("Failed to connect to gar (is it running?): {}", e))?;

    // Send request
    let json = serde_json::to_string(&request)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;
    writeln!(stream, "{}", json)
        .map_err(|e| format!("Failed to send request: {}", e))?;

    // Read response
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)
        .map_err(|e| format!("Failed to read response: {}", e))?;

    serde_json::from_str(&response)
        .map_err(|e| format!("Failed to parse response: {}", e))
}

fn main() {
    let cli = Cli::parse();

    let request = match cli.command {
        Command::Focus { direction } => {
            json!({ "command": "focus", "args": { "direction": direction } })
        }
        Command::Swap { direction } => {
            json!({ "command": "swap", "args": { "direction": direction } })
        }
        Command::Resize { direction, amount } => {
            json!({ "command": "resize", "args": { "direction": direction, "amount": amount } })
        }
        Command::Close => {
            json!({ "command": "close", "args": {} })
        }
        Command::Workspace { number } => {
            json!({ "command": "workspace", "args": { "number": number } })
        }
        Command::MoveToWorkspace { number } => {
            json!({ "command": "move_to_workspace", "args": { "number": number } })
        }
        Command::ToggleFloating => {
            json!({ "command": "toggle_floating", "args": {} })
        }
        Command::Equalize => {
            json!({ "command": "equalize", "args": {} })
        }
        Command::Reload => {
            json!({ "command": "reload", "args": {} })
        }
        Command::Exit => {
            json!({ "command": "exit", "args": {} })
        }
        Command::GetWorkspaces => {
            json!({ "command": "get_workspaces", "args": {} })
        }
        Command::GetFocused => {
            json!({ "command": "get_focused", "args": {} })
        }
        Command::GetTree => {
            json!({ "command": "get_tree", "args": {} })
        }
        Command::FocusMonitor { target } => {
            json!({ "command": "focus_monitor", "args": { "target": target } })
        }
        Command::MoveToMonitor { target } => {
            json!({ "command": "move_to_monitor", "args": { "target": target } })
        }
        Command::GetMonitors => {
            json!({ "command": "get_monitors", "args": {} })
        }
    };

    match send_command(request) {
        Ok(response) => {
            let success = response.get("success").and_then(|v| v.as_bool()).unwrap_or(false);

            if success {
                if let Some(data) = response.get("data") {
                    if !data.is_null() {
                        // Pretty print data
                        println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
                    }
                }
            } else {
                let error = response.get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
