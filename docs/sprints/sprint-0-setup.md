# Sprint 0: Project Setup

**Goal:** Buildable skeleton that connects to X server and appears in the display manager greeter.

## Objectives

- Initialize Rust project with proper workspace structure
- Establish logging and error handling patterns
- Connect to X server and become a window manager
- Create a stable event loop
- Make gar selectable from the login greeter

## Tasks

### 0.1 Initialize Cargo Project
- [ ] Create `Cargo.toml` with workspace members (gar, garctl)
- [ ] Set up `src/main.rs` entry point
- [ ] Set up `src/lib.rs` for library code
- [ ] Create module structure skeleton

**Dependencies to add:**
```toml
[dependencies]
x11rb = { version = "0.13", features = ["allow-unsafe-code"] }
tracing = "0.1"
tracing-subscriber = "0.3"
thiserror = "1.0"
```

### 0.2 Logging Infrastructure
- [ ] Initialize tracing subscriber in main
- [ ] Set up log levels (RUST_LOG environment variable)
- [ ] Add structured logging for key events
- [ ] Log to file for debugging sessions

### 0.3 X Server Connection
- [ ] Create `src/x11/mod.rs` module
- [ ] Create `src/x11/connection.rs`
- [ ] Connect to X server using `x11rb::connect()`
- [ ] Get root window and screen info
- [ ] Handle connection errors gracefully

**Key code pattern:**
```rust
use x11rb::connection::Connection;
use x11rb::rust_connection::RustConnection;

pub fn connect() -> Result<(RustConnection, usize), Error> {
    let (conn, screen_num) = x11rb::connect(None)?;
    Ok((conn, screen_num))
}
```

### 0.4 Become Window Manager
- [ ] Request SubstructureRedirect on root window
- [ ] Handle "another WM running" error gracefully
- [ ] Set up necessary event masks on root window

**Critical X11 concept:** Only ONE client can have SubstructureRedirect on the root window. This is how X11 ensures only one window manager runs.

```rust
let change = ChangeWindowAttributesAux::new()
    .event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY);
conn.change_window_attributes(root, &change)?.check()?;
```

### 0.5 Event Loop
- [ ] Create `src/x11/events.rs`
- [ ] Implement basic event loop structure
- [ ] Handle ConfigureRequest (pass through for now)
- [ ] Handle MapRequest (map windows for now)
- [ ] Log all received events
- [ ] Clean shutdown on SIGTERM/SIGINT

**Event loop pattern:**
```rust
loop {
    let event = conn.wait_for_event()?;
    match event {
        Event::ConfigureRequest(e) => handle_configure_request(&conn, e)?,
        Event::MapRequest(e) => handle_map_request(&conn, e)?,
        _ => tracing::trace!("Unhandled event: {:?}", event),
    }
}
```

### 0.6 Desktop Entry
- [ ] Create `gar.desktop` file
- [ ] Install to `/usr/share/xsessions/` (or document install location)
- [ ] Test selection from GDM/LightDM/SDDM

**Desktop entry content:**
```ini
[Desktop Entry]
Name=gar
Comment=Tiling window manager with smart splits
Exec=gar
Type=XSession
```

## Acceptance Criteria

1. `cargo build` succeeds without errors
2. `cargo run` connects to X server (or fails gracefully if one isn't available)
3. Running in a nested X session (Xephyr) shows gar as the WM
4. `gar.desktop` can be created and would appear in a greeter
5. Existing windows are not lost (ConfigureRequest/MapRequest passthrough)
6. Logs show event activity

## Testing Strategy

**Manual testing with Xephyr:**
```bash
# Terminal 1: Start nested X server
Xephyr -br -ac -noreset -screen 1280x720 :1

# Terminal 2: Run gar in nested session
DISPLAY=:1 cargo run

# Terminal 3: Launch apps in nested session
DISPLAY=:1 xterm
```

## Notes

- Keep this sprint minimal - just enough to have a working foundation
- Don't implement actual window management yet (Sprint 1)
- Focus on correctness over features
- Document any X11 quirks encountered

## Resources

- [x11rb documentation](https://docs.rs/x11rb/latest/x11rb/)
- [X11 Protocol Reference](https://www.x.org/releases/current/doc/xproto/x11protocol.html)
- [ICCCM Spec](https://www.x.org/releases/X11R7.6/doc/xorg-docs/specs/ICCCM/icccm.html)
