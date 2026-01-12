# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**gar** is a Rust X11 tiling window manager with smart BSP (binary space partitioning) tree layout, i3-compatible IPC, and Lua scripting for configuration.

Workspace structure:
- `gar/` - Main window manager crate (~6,300 lines)
- `garctl/` - CLI control tool for managing gar via IPC

## Build Commands

```bash
# Development build
cargo build

# Release build (required for start-gar.sh)
cargo build --release

# Build specific package
cargo build -p gar
cargo build -p garctl

# Lint and format
cargo clippy
cargo fmt
```

Requires X11 development headers. On NixOS/Nix, use:
```bash
nix-shell --run 'cargo build --release'
```

## Running and Testing

**From TTY (recommended for real X session):**
```bash
./start-gar.sh
```

**Nested X session (safe testing without switching sessions):**
```bash
Xephyr -br -ac -noreset -screen 1280x720 :1 &
DISPLAY=:1 cargo run
```

**Panic exit:** `Super+Shift+Escape`

**Logs:** `/tmp/gar.log` and stdout. Control with `RUST_LOG` or `GAR_LOG` environment variables.

## Architecture

### Event-Driven Design

```
X11 Event Loop (x11/events.rs)
    ↓
handle_event() dispatch
    ├→ MapRequest       → manage_window()
    ├→ KeyPress         → keybind lookup → action
    ├→ ButtonPress      → drag/focus handling
    ├→ UnmapNotify      → unmanage_window()
    ├→ RandR events     → refresh_monitors()
    └→ ClientMessage    → EWMH requests
    ↓
apply_layout() → X11 configure calls → flush
```

### Core Modules

| Module | Purpose |
|--------|---------|
| `core/mod.rs` | WindowManager struct - central state and event orchestration |
| `core/tree.rs` | BSP tree for smart tiling layout |
| `core/workspace.rs` | Workspace management (10 workspaces, i3-style) |
| `x11/events.rs` | Event loop and X11 event handlers |
| `x11/connection.rs` | X11 connection, atoms, EWMH/ICCCM |
| `config/lua.rs` | Lua API setup and config integration |
| `ipc/server.rs` | Custom IPC (Unix socket, JSON protocol) |
| `ipc/i3_server.rs` | i3-compatible IPC for polybar integration |

### Window States

- **Tiled** - Managed by BSP tree
- **Floating** - Separate list, rendered above tiled windows
- **Fullscreen** - Covers entire monitor, restores previous state on toggle

### IPC

Two socket servers run simultaneously:
- Custom protocol at `$XDG_RUNTIME_DIR/gar.sock` (or `/tmp/gar.sock`)
- i3-compatible protocol for polybar at separate socket

Control via `garctl`:
```bash
garctl focus left
garctl workspace 3
garctl get-workspaces
```

## Configuration

Lua config at `~/.config/gar/init.lua`. Default config with keybinds: `gar/config/default.lua`

Key Lua API functions:
- `gar.bind(key, action)` - Bind key to action
- `gar.set(option, value)` - Set config option
- `gar.exec(cmd)` / `gar.exec_once(cmd)` - Execute commands
- `gar.focus(dir)`, `gar.swap(dir)`, `gar.resize(dir, amount)` - Window operations
- `gar.workspace(n)`, `gar.move_to_workspace(n)` - Workspace operations
- `gar.reload`, `gar.exit` - WM control

Mod key: Use `"mod"` (Super) for real sessions, `"alt"` for nested Xephyr testing.

## Key Entry Points

1. `main.rs` - Logging setup, X connection, WM initialization
2. `WindowManager::new()` in `core/mod.rs` - State initialization, monitor detection, IPC server startup
3. `WindowManager::run()` - Main event loop
4. `handle_event()` in `x11/events.rs` - Event dispatch to specific handlers
