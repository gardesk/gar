# Sprint 9: MVP Polish + Testing

**Goal:** Stable, documented MVP ready for release.

## Objectives

- Robust error handling throughout
- Crash recovery (restart in place)
- Documentation (man pages, examples)
- Integration testing
- Installation packaging

## Prerequisites

- Sprint 8 complete (EWMH, polish)

## Tasks

### 9.1 Error Handling Audit
- [ ] Review all `unwrap()` and `expect()` calls
- [ ] Replace with proper error handling
- [ ] Add context to errors with `thiserror`
- [ ] Log errors appropriately
- [ ] Never crash on X errors (use error handler)

```rust
// Bad
let window = self.get_window(id).unwrap();

// Good
let window = self.get_window(id)
    .ok_or_else(|| Error::WindowNotFound(id))?;

// With context
let tree = std::fs::read_to_string(&config_path)
    .map_err(|e| Error::ConfigLoad { path: config_path.clone(), source: e })?;
```

### 9.2 X Error Handling
- [ ] Set up X error handler
- [ ] Log X errors without crashing
- [ ] Handle "window destroyed" race conditions
- [ ] Recover from non-fatal errors

```rust
fn setup_error_handler(conn: &impl Connection) {
    // X11 error handling is synchronous in x11rb
    // Wrap operations that might fail due to window destruction
}

fn safe_configure_window(
    conn: &impl Connection,
    window: Window,
    aux: &ConfigureWindowAux,
) -> Result<()> {
    match conn.configure_window(window, aux)?.check() {
        Ok(_) => Ok(()),
        Err(e) if is_window_error(&e) => {
            tracing::debug!("Window {} no longer exists", window);
            Ok(()) // Not a fatal error
        }
        Err(e) => Err(e.into()),
    }
}
```

### 9.3 Crash Recovery
- [ ] Implement `--replace` flag
- [ ] Save state before exit/crash
- [ ] Restore state on restart
- [ ] Handle SIGTERM/SIGINT gracefully
- [ ] Support restart-in-place (Mod+Shift+R alternative)

```rust
fn save_state(&self, path: &Path) -> Result<()> {
    let state = SavedState {
        workspaces: self.workspaces.iter().map(|ws| {
            SavedWorkspace {
                name: ws.name.clone(),
                windows: ws.all_windows(),
            }
        }).collect(),
        focused: self.focused,
    };
    let json = serde_json::to_string(&state)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn restore_state(&mut self, path: &Path) -> Result<()> {
    let json = std::fs::read_to_string(path)?;
    let state: SavedState = serde_json::from_str(&json)?;
    // Restore workspaces and focus
    Ok(())
}

fn handle_signal(sig: i32) {
    match sig {
        SIGTERM | SIGINT => {
            // Save state and exit cleanly
            save_state(&state_path).ok();
            std::process::exit(0);
        }
        SIGUSR1 => {
            // Restart in place
            save_state(&state_path).ok();
            exec_self();
        }
        _ => {}
    }
}
```

### 9.4 Man Pages
- [ ] Create `docs/gar.1` (main man page)
- [ ] Create `docs/garctl.1` (CLI tool)
- [ ] Create `docs/gar-lua.5` (configuration)
- [ ] Document all keybinds and options

```man
.TH GAR 1 "2024" "gar" "User Commands"
.SH NAME
gar \- tiling window manager with smart splits
.SH SYNOPSIS
.B gar
[\fIOPTIONS\fR]
.SH DESCRIPTION
.B gar
is an X11 tiling window manager that automatically determines
the optimal split direction when creating new windows.
.SH OPTIONS
.TP
.B \-c, \-\-config FILE
Use alternate configuration file
.TP
.B \-\-replace
Replace currently running window manager
.TP
.B \-v, \-\-version
Print version and exit
.SH FILES
.TP
.I ~/.config/gar/init.lua
Default configuration file
.TP
.I $XDG_RUNTIME_DIR/gar.sock
IPC socket
.SH SEE ALSO
.BR garctl (1),
.BR gar-lua (5)
```

### 9.5 Example Configurations
- [ ] Create `examples/minimal.lua`
- [ ] Create `examples/i3-like.lua`
- [ ] Create `examples/gaps-and-borders.lua`
- [ ] Create `examples/polybar-integration.lua`
- [ ] Document each example

### 9.6 Integration Tests
- [ ] Set up test infrastructure with Xvfb
- [ ] Test window creation/destruction
- [ ] Test workspace switching
- [ ] Test keybind execution
- [ ] Test IPC commands
- [ ] Test config loading

```rust
// tests/integration.rs
use std::process::Command;

fn setup_xvfb() -> XvfbGuard {
    // Start Xvfb on :99
    // Return guard that kills on drop
}

#[test]
fn test_window_tiling() {
    let _xvfb = setup_xvfb();

    // Start gar
    let mut gar = Command::new("cargo")
        .args(["run", "--"])
        .env("DISPLAY", ":99")
        .spawn()
        .unwrap();

    // Wait for startup
    std::thread::sleep(Duration::from_millis(500));

    // Open windows
    Command::new("xterm")
        .env("DISPLAY", ":99")
        .spawn()
        .unwrap();

    // Verify via IPC
    let output = Command::new("garctl")
        .args(["get-tree"])
        .env("DISPLAY", ":99")
        .output()
        .unwrap();

    let tree: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Assert tree structure

    gar.kill().unwrap();
}
```

### 9.7 Installation Script
- [ ] Create `install.sh` for manual install
- [ ] Create Makefile with install target
- [ ] Document dependencies
- [ ] Test on fresh system

```bash
#!/bin/bash
# install.sh

set -e

PREFIX=${PREFIX:-/usr/local}

cargo build --release

install -Dm755 target/release/gar "$PREFIX/bin/gar"
install -Dm755 target/release/garctl "$PREFIX/bin/garctl"
install -Dm644 gar.desktop "$PREFIX/share/xsessions/gar.desktop"
install -Dm644 docs/gar.1 "$PREFIX/share/man/man1/gar.1"
install -Dm644 docs/garctl.1 "$PREFIX/share/man/man1/garctl.1"

echo "Installation complete!"
```

### 9.8 AUR Package (optional)
- [ ] Create PKGBUILD
- [ ] Test in clean chroot
- [ ] Submit to AUR

```bash
# PKGBUILD
pkgname=gar
pkgver=0.1.0
pkgrel=1
pkgdesc="Tiling window manager with smart splits"
arch=('x86_64')
url="https://github.com/youruser/gar"
license=('MIT')
depends=('libxcb' 'lua')
makedepends=('rust' 'cargo')

build() {
    cd "$srcdir/$pkgname-$pkgver"
    cargo build --release
}

package() {
    cd "$srcdir/$pkgname-$pkgver"
    install -Dm755 target/release/gar "$pkgdir/usr/bin/gar"
    install -Dm755 target/release/garctl "$pkgdir/usr/bin/garctl"
    install -Dm644 gar.desktop "$pkgdir/usr/share/xsessions/gar.desktop"
}
```

### 9.9 README and Documentation
- [ ] Update README with features, screenshots
- [ ] Add CONTRIBUTING.md
- [ ] Add LICENSE file
- [ ] Document known issues/limitations

### 9.10 Final Testing Checklist
- [ ] Fresh install works
- [ ] All documented keybinds work
- [ ] Polybar integration works
- [ ] Rofi integration works
- [ ] Multi-monitor works
- [ ] Config reload works
- [ ] No memory leaks (valgrind)
- [ ] No crash on stress test

## Acceptance Criteria

1. No panics in release build under normal use
2. Man pages installed and accessible
3. Example configs provided and documented
4. Integration tests pass
5. Install script works on fresh Arch/Ubuntu
6. README provides clear getting started guide

## Testing Strategy

```bash
# Full test suite
cargo test

# Integration tests (requires Xvfb)
cargo test --test integration

# Memory check
valgrind --leak-check=full target/release/gar

# Stress test
for i in {1..100}; do
    DISPLAY=:1 xterm &
done
# Then close all, verify no issues
```

## MVP Feature Summary

After Sprint 9, gar will have:

- Smart split detection (the core feature)
- Full keyboard navigation
- 10 workspaces
- Multi-monitor support
- Floating window support
- Lua configuration
- IPC system with garctl
- EWMH compliance
- Configurable borders and gaps
- Documentation and examples

## Post-MVP Ideas (Future Sprints)

- Scratchpad windows
- Tabbed/stacked layouts
- Animations (fade, slide)
- Built-in bar (optional)
- Session save/restore
- Marks (like vim marks)
- Modes (resize mode, move mode)
- More layout algorithms
