# Sprint 6: IPC System

**Goal:** External control via Unix socket with JSON protocol.

## Objectives

- Unix socket server for IPC
- JSON-based command/response protocol
- Command-line tool (`garctl`) for interaction
- Event subscription for external tools

## Prerequisites

- Sprint 5 complete (floating windows)

## Protocol Design

### Message Format

```json
// Command (client -> server)
{
    "type": "command",
    "command": "focus",
    "args": { "direction": "left" }
}

// Response (server -> client)
{
    "type": "response",
    "success": true,
    "data": { /* optional result */ }
}

// Error response
{
    "type": "response",
    "success": false,
    "error": "Window not found"
}

// Event (server -> subscribed clients)
{
    "type": "event",
    "event": "window_focus",
    "data": { "window_id": 12345, "title": "Terminal" }
}
```

### Commands

| Command | Args | Description |
|---------|------|-------------|
| focus | direction | Focus window in direction |
| swap | direction | Swap with window in direction |
| resize | direction, amount | Resize split |
| close | - | Close focused window |
| workspace | number | Switch to workspace |
| move_to_workspace | number | Move window to workspace |
| toggle_floating | - | Toggle floating state |
| reload | - | Reload configuration |
| exit | - | Exit gar |
| get_tree | - | Get window tree |
| get_workspaces | - | Get workspace info |
| get_focused | - | Get focused window |
| subscribe | events[] | Subscribe to events |

### Events

| Event | Data | Description |
|-------|------|-------------|
| window_new | window info | New window created |
| window_close | window_id | Window closed |
| window_focus | window info | Focus changed |
| window_move | window_id, workspace | Window moved |
| workspace_focus | workspace info | Workspace changed |
| mode | mode name | Mode changed (later) |

## Tasks

### 6.1 Socket Server Setup
- [ ] Create `src/ipc/mod.rs`, `server.rs`, `protocol.rs`
- [ ] Add tokio dependency for async I/O
- [ ] Create Unix socket at `$XDG_RUNTIME_DIR/gar.sock`
- [ ] Handle multiple concurrent clients
- [ ] Clean up socket on exit

```rust
use tokio::net::{UnixListener, UnixStream};

pub struct IpcServer {
    listener: UnixListener,
    clients: Vec<Client>,
}

impl IpcServer {
    pub async fn new() -> Result<Self> {
        let path = std::env::var("XDG_RUNTIME_DIR")
            .map(|dir| format!("{}/gar.sock", dir))
            .unwrap_or_else(|_| "/tmp/gar.sock".to_string());

        // Remove existing socket
        let _ = std::fs::remove_file(&path);

        let listener = UnixListener::bind(&path)?;
        Ok(Self { listener, clients: Vec::new() })
    }
}
```

### 6.2 Protocol Types
- [ ] Define message types with serde
- [ ] Implement JSON serialization
- [ ] Handle malformed messages gracefully

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    #[serde(rename = "command")]
    Command { command: String, args: serde_json::Value },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum Response {
    #[serde(rename = "response")]
    Success { success: bool, data: Option<serde_json::Value> },
    #[serde(rename = "response")]
    Error { success: bool, error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "event")]
    Event { event: String, data: serde_json::Value },
}
```

### 6.3 Command Dispatch
- [ ] Parse incoming commands
- [ ] Map to window manager actions
- [ ] Execute and return result
- [ ] Handle unknown commands

```rust
impl IpcServer {
    fn dispatch(&self, wm: &mut WindowManager, cmd: &str, args: Value) -> Response {
        match cmd {
            "focus" => {
                let direction = args["direction"].as_str().unwrap();
                wm.focus_direction(direction.parse()?)?;
                Response::success(None)
            }
            "get_tree" => {
                let tree = wm.get_tree_json();
                Response::success(Some(tree))
            }
            _ => Response::error(format!("Unknown command: {}", cmd)),
        }
    }
}
```

### 6.4 Query Commands
- [ ] `get_tree` - return workspace trees as JSON
- [ ] `get_workspaces` - return workspace list
- [ ] `get_focused` - return focused window info
- [ ] `get_outputs` - return monitor info (for multi-monitor)

```rust
fn get_tree_json(&self) -> Value {
    json!({
        "workspaces": self.workspaces.iter().map(|ws| {
            json!({
                "name": ws.name,
                "focused": ws.focused,
                "nodes": tree_to_json(&ws.tree),
            })
        }).collect::<Vec<_>>()
    })
}
```

### 6.5 Event Subscription
- [ ] Track subscribed clients per event type
- [ ] Broadcast events to subscribers
- [ ] Handle client disconnection
- [ ] Implement subscription command

```rust
struct Client {
    stream: UnixStream,
    subscriptions: HashSet<String>,
}

impl IpcServer {
    fn broadcast_event(&mut self, event: &str, data: Value) {
        let msg = Event { event: event.into(), data };
        let json = serde_json::to_string(&msg).unwrap();

        self.clients.retain(|client| {
            if client.subscriptions.contains(event) {
                client.stream.try_write(json.as_bytes()).is_ok()
            } else {
                true
            }
        });
    }
}
```

### 6.6 Integration with Event Loop
- [ ] Run IPC server alongside X event loop
- [ ] Use tokio runtime or poll-based approach
- [ ] Handle commands without blocking X events
- [ ] Thread-safe communication with WM state

```rust
// Option 1: Tokio with channels
fn main() {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(100);
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(100);

    // Spawn IPC server task
    tokio::spawn(async move {
        ipc_server.run(cmd_tx, event_rx).await;
    });

    // X event loop
    loop {
        // Check for IPC commands
        while let Ok(cmd) = cmd_rx.try_recv() {
            handle_ipc_command(cmd);
        }

        // Handle X events
        let event = conn.wait_for_event()?;
        handle_x_event(event);

        // Broadcast events
        event_tx.send(/* ... */);
    }
}
```

### 6.7 garctl CLI Tool
- [ ] Create `garctl/src/main.rs`
- [ ] Connect to gar socket
- [ ] Send commands from CLI args
- [ ] Print responses
- [ ] Support event monitoring mode

```rust
// garctl/src/main.rs
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Parser)]
enum Command {
    Focus { direction: String },
    Workspace { number: u32 },
    GetTree,
    Subscribe { events: Vec<String> },
    // ...
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut socket = UnixStream::connect(get_socket_path())?;

    let request = match cli.command {
        Command::Focus { direction } => {
            json!({ "type": "command", "command": "focus", "args": { "direction": direction } })
        }
        // ...
    };

    socket.write_all(serde_json::to_string(&request)?.as_bytes())?;

    let mut response = String::new();
    socket.read_to_string(&mut response)?;
    println!("{}", response);

    Ok(())
}
```

## garctl Usage

```bash
# Focus direction
garctl focus left
garctl focus right

# Workspaces
garctl workspace 2
garctl move-to-workspace 3

# Queries
garctl get-tree
garctl get-workspaces
garctl get-focused

# Events (stays open, prints events)
garctl subscribe window_focus workspace_focus
```

## Acceptance Criteria

1. Socket created at `$XDG_RUNTIME_DIR/gar.sock`
2. `garctl` can send commands and receive responses
3. All WM actions available via IPC
4. Query commands return proper JSON
5. Event subscription works for focus/workspace changes
6. Multiple concurrent clients supported

## Testing Strategy

```bash
# Start gar
DISPLAY=:1 cargo run

# Test command
garctl focus left  # Should return success

# Test query
garctl get-tree | jq .  # Should show tree structure

# Test events
garctl subscribe window_focus &
# Open/focus windows, events should print
```

## Notes

- Socket permissions should be user-only (0600)
- Consider i3-compatible message format for polybar compatibility
- Add timeout for client reads to prevent blocking
- Log all IPC activity for debugging
