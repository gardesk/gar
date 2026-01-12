#!/bin/bash
# gar session wrapper - sets up environment before starting gar

GAR_DIR="$(cd "$(dirname "$0")" && pwd)"

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

# Launch compositor before WM (for proper screen repainting)
# gar generates picom.conf on startup and signals picom to reload
if command -v picom &> /dev/null; then
    if [[ -f ~/.config/gar/picom.conf ]]; then
        picom -b --config ~/.config/gar/picom.conf &
    else
        # First run: start with GLX backend, gar will generate config and signal reload
        picom -b --backend glx &
    fi
    sleep 0.1
fi

# Set log level
export GAR_LOG=info

# Launch polybar after gar starts (needs i3 IPC socket)
(sleep 0.5 && ~/.config/polybar/launch.sh) &

# Start gar
exec /home/mfwolffe/GithubOrgs/tenseleyFlow/gar/target/release/gar
