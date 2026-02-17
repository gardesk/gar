#!/bin/bash
# gar session wrapper - sets up environment before starting gar
# This script is typically installed to /usr/local/share/gar/gar-session.sh

# Find gar binary - check common locations (user local first for dev overrides)
GAR_BIN="${GAR_BIN:-}"
if [ -z "$GAR_BIN" ]; then
    for path in "$HOME/.local/bin/gar" /usr/local/bin/gar /usr/bin/gar; do
        if [ -x "$path" ]; then
            GAR_BIN="$path"
            break
        fi
    done
fi

# Fallback to PATH lookup
if [ -z "$GAR_BIN" ] || [ ! -x "$GAR_BIN" ]; then
    GAR_BIN="$(command -v gar 2>/dev/null || true)"
fi

if [ -z "$GAR_BIN" ] || [ ! -x "$GAR_BIN" ]; then
    echo "ERROR: gar binary not found. Please ensure gar is installed." >&2
    exit 1
fi

# Optional: configure monitor layout
# Uncomment and customize for your setup:
# xrandr --output eDP-1 --mode 2880x1800 --pos 0x0
# xrandr --output DP-1 --mode 1920x1080 --pos 0x0 \
#   --output HDMI-1 --mode 2560x1440 --pos 1920x0

# Ensure gar config directory exists
mkdir -p ~/.config/gar

# ═══════════════════════════════════════════════════════════════════
# SYSTEMD SESSION SETUP - Required for user services like garbg
# Pattern from sway-systemd, i3-session, and ArchWiki systemd/User
# ═══════════════════════════════════════════════════════════════════

# Import DISPLAY and XAUTHORITY to systemd user session
# This allows user services to connect to X11
systemctl --user import-environment DISPLAY XAUTHORITY

# Also update D-Bus activation environment (for D-Bus services)
if command -v dbus-update-activation-environment &> /dev/null; then
    dbus-update-activation-environment DISPLAY XAUTHORITY
fi

# Start gar-session.target - this binds to graphical-session.target
# (graphical-session.target has RefuseManualStart=yes, so we use our own target)
systemctl --user start gar-session.target

# ═══════════════════════════════════════════════════════════════════

# Compositor is now managed by gar itself via start_compositor()
# based on the "compositor" setting in init.lua ("picom", "garchomp", or "none")
# Do NOT start picom here - it causes dual-compositor conflicts when garchomp is selected

# Set log level
export GAR_LOG=info

# garbar is now launched natively by gar when gar.bar is configured in init.lua
# (Legacy polybar launch removed - use gar.bar config instead)

# Start gar
exec "$GAR_BIN"
