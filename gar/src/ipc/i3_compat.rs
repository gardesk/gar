//! i3 IPC wire protocol implementation.
//!
//! Binary format: "i3-ipc" (6 bytes) + length (u32 LE) + type (u32 LE) + JSON payload
//! Events have the high bit set: 0x80000000 | message_type

use std::io::{self, Read, Write};

/// Magic string that prefixes all i3 IPC messages.
pub const I3_MAGIC: &[u8; 6] = b"i3-ipc";

/// Header size: 6 (magic) + 4 (length) + 4 (type) = 14 bytes.
pub const HEADER_SIZE: usize = 14;

/// High bit mask for event messages.
pub const EVENT_MASK: u32 = 0x80000000;

/// i3 IPC message types (requests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    RunCommand = 0,
    GetWorkspaces = 1,
    Subscribe = 2,
    GetOutputs = 3,
    GetTree = 4,
    GetMarks = 5,
    GetBarConfig = 6,
    GetVersion = 7,
    GetBindingModes = 8,
    GetConfig = 9,
    SendTick = 10,
    Sync = 11,
    GetBindingState = 12,
}

impl MessageType {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0 => Some(Self::RunCommand),
            1 => Some(Self::GetWorkspaces),
            2 => Some(Self::Subscribe),
            3 => Some(Self::GetOutputs),
            4 => Some(Self::GetTree),
            5 => Some(Self::GetMarks),
            6 => Some(Self::GetBarConfig),
            7 => Some(Self::GetVersion),
            8 => Some(Self::GetBindingModes),
            9 => Some(Self::GetConfig),
            10 => Some(Self::SendTick),
            11 => Some(Self::Sync),
            12 => Some(Self::GetBindingState),
            _ => None,
        }
    }
}

/// i3 IPC event types (responses with high bit set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EventType {
    Workspace = 0,
    Output = 1,
    Mode = 2,
    Window = 3,
    BarconfigUpdate = 4,
    Binding = 5,
    Shutdown = 6,
    Tick = 7,
}

impl EventType {
    /// Get the wire format value (with high bit set).
    pub fn to_wire(&self) -> u32 {
        EVENT_MASK | (*self as u32)
    }
}

/// A parsed i3 IPC message.
#[derive(Debug)]
pub struct I3Message {
    pub msg_type: u32,
    pub payload: Vec<u8>,
}

impl I3Message {
    pub fn new(msg_type: u32, payload: Vec<u8>) -> Self {
        Self { msg_type, payload }
    }

    /// Check if this is an event message (high bit set).
    pub fn is_event(&self) -> bool {
        self.msg_type & EVENT_MASK != 0
    }

    /// Get the message type without the event flag.
    pub fn base_type(&self) -> u32 {
        self.msg_type & !EVENT_MASK
    }

    /// Get payload as string (for JSON parsing).
    pub fn payload_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.payload)
    }
}

/// Read an i3 IPC message from a stream.
/// Returns None if the stream would block or is closed.
pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Option<I3Message>> {
    // Read header
    let mut header = [0u8; HEADER_SIZE];
    match reader.read_exact(&mut header) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    // Verify magic
    if &header[0..6] != I3_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid i3 IPC magic",
        ));
    }

    // Parse length and type (little-endian)
    let length = u32::from_le_bytes([header[6], header[7], header[8], header[9]]) as usize;
    let msg_type = u32::from_le_bytes([header[10], header[11], header[12], header[13]]);

    // Sanity check length (max 16MB)
    if length > 16 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Message too large",
        ));
    }

    // Read payload
    let mut payload = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut payload)?;
    }

    Ok(Some(I3Message::new(msg_type, payload)))
}

/// Write an i3 IPC message to a stream.
pub fn write_message<W: Write>(writer: &mut W, msg_type: u32, payload: &[u8]) -> io::Result<()> {
    let length = payload.len() as u32;

    // Write header
    writer.write_all(I3_MAGIC)?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&msg_type.to_le_bytes())?;

    // Write payload
    if !payload.is_empty() {
        writer.write_all(payload)?;
    }

    writer.flush()
}

/// Write an i3 IPC response (same type as request).
pub fn write_response<W: Write>(writer: &mut W, msg_type: u32, json: &str) -> io::Result<()> {
    write_message(writer, msg_type, json.as_bytes())
}

/// Write an i3 IPC event (with high bit set).
pub fn write_event<W: Write>(writer: &mut W, event_type: EventType, json: &str) -> io::Result<()> {
    write_message(writer, event_type.to_wire(), json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_write_read_roundtrip() {
        let mut buf = Vec::new();
        write_message(&mut buf, 1, b"{\"test\": true}").unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_message(&mut cursor).unwrap().unwrap();

        assert_eq!(msg.msg_type, 1);
        assert_eq!(msg.payload_str().unwrap(), "{\"test\": true}");
    }

    #[test]
    fn test_event_type_wire_format() {
        assert_eq!(EventType::Workspace.to_wire(), 0x80000000);
        assert_eq!(EventType::Output.to_wire(), 0x80000001);
    }
}
