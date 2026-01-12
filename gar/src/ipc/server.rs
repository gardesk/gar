use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use serde_json::Value;

use super::protocol::{Event, Request, Response};

/// Result of reading from a client
enum ReadResult {
    Request(Request),
    WouldBlock,
    Disconnected,
}

/// A connected IPC client
struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    subscriptions: HashSet<String>,
}

impl Client {
    fn new(stream: UnixStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            subscriptions: HashSet::new(),
        })
    }

    fn read_request(&mut self) -> ReadResult {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => ReadResult::Disconnected,
            Ok(_) => {
                match serde_json::from_str(&line) {
                    Ok(req) => ReadResult::Request(req),
                    Err(_) => ReadResult::WouldBlock, // Malformed, ignore
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => ReadResult::WouldBlock,
            Err(_) => ReadResult::Disconnected,
        }
    }

    fn send_response(&mut self, response: &Response) -> std::io::Result<()> {
        let json = serde_json::to_string(response)?;
        writeln!(self.stream, "{}", json)?;
        self.stream.flush()
    }

    fn send_event(&mut self, event: &Event) -> std::io::Result<()> {
        let json = serde_json::to_string(event)?;
        writeln!(self.stream, "{}", json)?;
        self.stream.flush()
    }
}

/// IPC server for external control
pub struct IpcServer {
    listener: UnixListener,
    clients: Vec<Client>,
    socket_path: PathBuf,
}

impl IpcServer {
    /// Create a new IPC server
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

        tracing::info!("IPC server listening on {:?}", socket_path);

        Ok(Self {
            listener,
            clients: Vec::new(),
            socket_path,
        })
    }

    /// Get the socket path
    fn socket_path() -> PathBuf {
        std::env::var("XDG_RUNTIME_DIR")
            .map(|dir| PathBuf::from(dir).join("gar.sock"))
            .unwrap_or_else(|_| PathBuf::from("/tmp/gar.sock"))
    }

    /// Accept new connections (non-blocking)
    pub fn accept_connections(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    tracing::debug!("New IPC client connected");
                    match Client::new(stream) {
                        Ok(client) => self.clients.push(client),
                        Err(e) => tracing::warn!("Failed to setup client: {}", e),
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    tracing::warn!("Failed to accept connection: {}", e);
                    break;
                }
            }
        }
    }

    /// Process incoming requests from all clients
    /// Returns a list of (client_index, request) pairs
    pub fn poll_requests(&mut self) -> Vec<(usize, Request)> {
        let mut requests = Vec::new();
        let mut disconnected = Vec::new();

        for (i, client) in self.clients.iter_mut().enumerate() {
            match client.read_request() {
                ReadResult::Request(req) => requests.push((i, req)),
                ReadResult::Disconnected => disconnected.push(i),
                ReadResult::WouldBlock => {}
            }
        }

        // Remove disconnected clients (in reverse order to preserve indices)
        for i in disconnected.into_iter().rev() {
            tracing::debug!("IPC client disconnected");
            self.clients.remove(i);
        }

        requests
    }

    /// Send a response to a specific client
    pub fn send_response(&mut self, client_idx: usize, response: Response) {
        if let Some(client) = self.clients.get_mut(client_idx) {
            if let Err(e) = client.send_response(&response) {
                tracing::warn!("Failed to send response: {}", e);
            }
        }
    }

    /// Subscribe a client to events
    pub fn subscribe(&mut self, client_idx: usize, events: Vec<String>) {
        if let Some(client) = self.clients.get_mut(client_idx) {
            for event in events {
                client.subscriptions.insert(event);
            }
        }
    }

    /// Broadcast an event to all subscribed clients
    pub fn broadcast_event(&mut self, event_name: &str, data: Value) {
        let event = Event::new(event_name, data);
        let mut disconnected = Vec::new();

        for (i, client) in self.clients.iter_mut().enumerate() {
            if client.subscriptions.contains(event_name) || client.subscriptions.contains("*") {
                if let Err(_) = client.send_event(&event) {
                    disconnected.push(i);
                }
            }
        }

        // Remove failed clients
        for i in disconnected.into_iter().rev() {
            self.clients.remove(i);
        }
    }

    /// Get client count
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Clean up socket file
        let _ = std::fs::remove_file(&self.socket_path);
        tracing::debug!("IPC server shutdown, socket removed");
    }
}
