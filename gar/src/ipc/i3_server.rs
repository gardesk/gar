//! i3-compatible IPC server for polybar and other i3 ecosystem tools.
//!
//! Implements a subset of i3's IPC protocol:
//! - GET_WORKSPACES (type 1)
//! - SUBSCRIBE (type 2)
//! - GET_OUTPUTS (type 3)
//! - GET_VERSION (type 7)
//!
//! Socket path is set via I3SOCK environment variable.

use std::collections::HashSet;
use std::io::{BufReader, BufWriter};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::i3_compat::{read_message, write_event, write_response, EventType, I3Message};

/// Result of reading from a client.
enum ReadResult {
    Message(I3Message),
    WouldBlock,
    Disconnected,
}

/// Timeout for clients that have never sent a message and have no subscriptions.
const IDLE_CLIENT_TIMEOUT: Duration = Duration::from_secs(60);

/// A connected i3 IPC client.
struct I3Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    writer: BufWriter<UnixStream>,
    subscriptions: HashSet<String>,
    /// When this client connected.
    created_at: Instant,
    /// When we last received a message from this client.
    last_activity: Option<Instant>,
}

impl I3Client {
    fn new(stream: UnixStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        let reader = BufReader::new(stream.try_clone()?);
        let writer = BufWriter::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            writer,
            subscriptions: HashSet::new(),
            created_at: Instant::now(),
            last_activity: None,
        })
    }

    /// Check if this client is stale and should be cleaned up.
    /// A client is stale if it has never sent a message, has no subscriptions,
    /// and has been connected for longer than IDLE_CLIENT_TIMEOUT.
    fn is_stale(&self) -> bool {
        // Clients with subscriptions are waiting for events - keep them
        if !self.subscriptions.is_empty() {
            return false;
        }
        // Clients that have sent messages are active - keep them
        if self.last_activity.is_some() {
            return false;
        }
        // New clients that haven't done anything yet - check timeout
        self.created_at.elapsed() > IDLE_CLIENT_TIMEOUT
    }

    fn read_message(&mut self) -> ReadResult {
        match read_message(&mut self.reader) {
            Ok(Some(msg)) => {
                self.last_activity = Some(Instant::now());
                ReadResult::Message(msg)
            }
            Ok(None) => ReadResult::WouldBlock,
            Err(e) => {
                tracing::debug!("i3 IPC client read error: {}", e);
                ReadResult::Disconnected
            }
        }
    }

    fn send_response(&mut self, msg_type: u32, json: &str) -> std::io::Result<()> {
        write_response(&mut self.writer, msg_type, json)
    }

    fn send_event(&mut self, event_type: EventType, json: &str) -> std::io::Result<()> {
        write_event(&mut self.writer, event_type, json)
    }

    fn is_subscribed(&self, event: &str) -> bool {
        self.subscriptions.contains(event)
    }

    fn subscribe(&mut self, events: Vec<String>) {
        for event in events {
            self.subscriptions.insert(event);
        }
    }
}

/// i3-compatible IPC server.
pub struct I3IpcServer {
    listener: UnixListener,
    clients: Vec<I3Client>,
    socket_path: PathBuf,
}

impl I3IpcServer {
    /// Create a new i3-compatible IPC server.
    /// Sets the I3SOCK environment variable for client discovery.
    pub fn new() -> std::io::Result<Self> {
        let socket_path = Self::socket_path();

        // Remove existing socket
        let _ = std::fs::remove_file(&socket_path);

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        listener.set_nonblocking(true)?;

        // Set socket permissions to user-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        }

        // Set I3SOCK environment variable so polybar and other tools can find us
        // SAFETY: We're setting this at startup before any threads are spawned
        unsafe { std::env::set_var("I3SOCK", &socket_path); }

        tracing::info!("i3-compatible IPC server listening on {:?}", socket_path);
        tracing::info!("I3SOCK={}", socket_path.display());

        Ok(Self {
            listener,
            clients: Vec::new(),
            socket_path,
        })
    }

    /// Get the socket path.
    fn socket_path() -> PathBuf {
        std::env::var("XDG_RUNTIME_DIR")
            .map(|dir| PathBuf::from(dir).join("gar-i3.sock"))
            .unwrap_or_else(|_| PathBuf::from("/tmp/gar-i3.sock"))
    }

    /// Accept new connections (non-blocking).
    pub fn accept_connections(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    tracing::debug!("New i3 IPC client connected");
                    match I3Client::new(stream) {
                        Ok(client) => self.clients.push(client),
                        Err(e) => tracing::warn!("Failed to setup i3 IPC client: {}", e),
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    tracing::warn!("Failed to accept i3 IPC connection: {}", e);
                    break;
                }
            }
        }
    }

    /// Process incoming requests from all clients.
    /// Returns a list of (client_index, message) pairs.
    /// Also cleans up stale/disconnected clients.
    pub fn poll_requests(&mut self) -> Vec<(usize, I3Message)> {
        let mut requests = Vec::new();
        let mut to_remove = Vec::new();

        for (i, client) in self.clients.iter_mut().enumerate() {
            match client.read_message() {
                ReadResult::Message(msg) => requests.push((i, msg)),
                ReadResult::Disconnected => to_remove.push(i),
                ReadResult::WouldBlock => {
                    // Check if this client is stale (never sent anything, no subscriptions)
                    if client.is_stale() {
                        tracing::debug!("Cleaning up stale i3 IPC client (no activity for {:?})", IDLE_CLIENT_TIMEOUT);
                        to_remove.push(i);
                    }
                }
            }
        }

        // Remove disconnected/stale clients (in reverse order to preserve indices)
        for i in to_remove.into_iter().rev() {
            self.clients.remove(i);
        }

        requests
    }

    /// Send a response to a specific client.
    pub fn send_response(&mut self, client_idx: usize, msg_type: u32, json: &str) {
        if let Some(client) = self.clients.get_mut(client_idx) {
            if let Err(e) = client.send_response(msg_type, json) {
                tracing::warn!("Failed to send i3 IPC response: {}", e);
            }
        }
    }

    /// Subscribe a client to events.
    pub fn subscribe(&mut self, client_idx: usize, events: Vec<String>) {
        if let Some(client) = self.clients.get_mut(client_idx) {
            client.subscribe(events);
        }
    }

    /// Broadcast a workspace event to all subscribed clients.
    pub fn broadcast_workspace_event(&mut self, json: &str) {
        let mut disconnected = Vec::new();

        for (i, client) in self.clients.iter_mut().enumerate() {
            if client.is_subscribed("workspace") {
                if client.send_event(EventType::Workspace, json).is_err() {
                    disconnected.push(i);
                }
            }
        }

        // Remove failed clients
        for i in disconnected.into_iter().rev() {
            self.clients.remove(i);
        }
    }

    /// Broadcast an output event to all subscribed clients.
    pub fn broadcast_output_event(&mut self, json: &str) {
        let mut disconnected = Vec::new();

        for (i, client) in self.clients.iter_mut().enumerate() {
            if client.is_subscribed("output") {
                if client.send_event(EventType::Output, json).is_err() {
                    disconnected.push(i);
                }
            }
        }

        // Remove failed clients
        for i in disconnected.into_iter().rev() {
            self.clients.remove(i);
        }
    }

    /// Get client count.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

impl Drop for I3IpcServer {
    fn drop(&mut self) {
        // Clean up socket file
        let _ = std::fs::remove_file(&self.socket_path);
        // Clear I3SOCK env var
        // SAFETY: We're removing this during cleanup, single-threaded context
        unsafe { std::env::remove_var("I3SOCK"); }
        tracing::debug!("i3 IPC server shutdown, socket removed");
    }
}

/// Build GET_WORKSPACES response JSON.
pub fn build_workspaces_json(workspaces: &[WorkspaceInfo]) -> String {
    serde_json::to_string(workspaces).unwrap_or_else(|_| "[]".to_string())
}

/// Build GET_OUTPUTS response JSON.
pub fn build_outputs_json(outputs: &[OutputInfo]) -> String {
    serde_json::to_string(outputs).unwrap_or_else(|_| "[]".to_string())
}

/// Build workspace event JSON.
pub fn build_workspace_event_json(change: &str, current: &WorkspaceInfo, old: Option<&WorkspaceInfo>) -> String {
    let event = WorkspaceEvent {
        change: change.to_string(),
        current: current.clone(),
        old: old.cloned(),
    };
    serde_json::to_string(&event).unwrap_or_else(|_| r#"{"change":"focus"}"#.to_string())
}

/// Build output event JSON.
pub fn build_output_event_json() -> String {
    r#"{"change":"unspecified"}"#.to_string()
}

/// Build GET_VERSION response JSON.
pub fn build_version_json() -> String {
    let version = VersionInfo {
        major: 0,
        minor: 1,
        patch: 0,
        human_readable: "gar 0.1.0 (i3-compat)".to_string(),
        loaded_config_file_name: "~/.config/gar/init.lua".to_string(),
    };
    serde_json::to_string(&version).unwrap_or_else(|_| r#"{"human_readable":"gar"}"#.to_string())
}

/// Build SUBSCRIBE success response JSON.
pub fn build_subscribe_success_json() -> String {
    r#"{"success":true}"#.to_string()
}

// ============================================================================
// Data structures for JSON serialization (i3-compatible)
// ============================================================================

use serde::{Deserialize, Serialize};

/// Workspace info for GET_WORKSPACES response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: i64,
    pub num: i32,
    pub name: String,
    pub visible: bool,
    pub focused: bool,
    pub urgent: bool,
    pub rect: Rect,
    pub output: String,
}

/// Output info for GET_OUTPUTS response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputInfo {
    pub name: String,
    pub active: bool,
    pub primary: bool,
    pub current_workspace: Option<String>,
    pub rect: Rect,
}

/// Rectangle for geometry info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Version info for GET_VERSION response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub human_readable: String,
    pub loaded_config_file_name: String,
}

/// Workspace event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEvent {
    pub change: String,
    pub current: WorkspaceInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<WorkspaceInfo>,
}
