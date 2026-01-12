use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Request from client to server
#[derive(Debug, Deserialize)]
pub struct Request {
    pub command: String,
    #[serde(default)]
    pub args: Value,
}

/// Response from server to client
#[derive(Debug, Serialize)]
pub struct Response {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn success(data: Option<Value>) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// Event broadcast to subscribed clients
#[derive(Debug, Serialize)]
pub struct Event {
    pub event: String,
    pub data: Value,
}

impl Event {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            event: event.into(),
            data,
        }
    }
}

/// Workspace info for queries
#[derive(Debug, Serialize)]
pub struct WorkspaceInfo {
    pub id: usize,
    pub name: String,
    pub focused: bool,
    pub tiled_count: usize,
    pub floating_count: usize,
}

/// Window info for queries
#[derive(Debug, Serialize)]
pub struct WindowInfo {
    pub id: u32,
    pub workspace: usize,
    pub floating: bool,
    pub focused: bool,
}
