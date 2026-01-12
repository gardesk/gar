#!/usr/bin/env bash
# Start gar window manager on a real X session
#
# IMPORTANT: Run this from a TTY, not from Hyprland!
#   1. Press Ctrl+Alt+F2 to switch to TTY2
#   2. Login
#   3. Run: ~/GithubOrgs/tenseleyFlow/gar/start-gar.sh
#
# PANIC MODE: Super+Shift+Escape to exit gar immediately
# BACKUP: Ctrl+Alt+F3 to switch to another TTY if gar freezes
#
# To return to Hyprland after exiting:
#   Press Ctrl+Alt+F1 (or wherever SDDM is running)

set -e

GAR_DIR="$(cd "$(dirname "$0")" && pwd)"
GAR_BIN="$GAR_DIR/target/release/gar"

# Check if gar is built
if [[ ! -x "$GAR_BIN" ]]; then
    echo "Error: gar not found at $GAR_BIN"
    echo "Run: nix-shell --run 'cargo build --release'"
    exit 1
fi

# Check if running from a TTY
if [[ -z "$XDG_SESSION_TYPE" ]] || [[ "$XDG_SESSION_TYPE" == "tty" ]]; then
    echo "Good: Running from TTY"
else
    echo "WARNING: You appear to be running from a graphical session."
    echo "For best results, switch to a TTY first (Ctrl+Alt+F2)"
    echo ""
    read -p "Continue anyway? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Find an available display
for i in 1 2 3 4 5; do
    if [[ ! -e "/tmp/.X$i-lock" ]]; then
        DISPLAY_NUM=":$i"
        break
    fi
done

if [[ -z "$DISPLAY_NUM" ]]; then
    echo "Error: No available display found"
    exit 1
fi

# Detect current VT
CURRENT_VT=$(tty | grep -o 'tty[0-9]*' | grep -o '[0-9]*' || echo "")
if [[ -z "$CURRENT_VT" ]]; then
    CURRENT_VT=7
fi

echo "========================================"
echo "  Starting gar window manager"
echo "========================================"
echo ""
echo "  PANIC MODE: Super+Shift+Escape"
echo "  TTY ESCAPE: Ctrl+Alt+F$((CURRENT_VT == 2 ? 3 : 2))"
echo ""
echo "  Display: $DISPLAY_NUM"
echo "  VT: $CURRENT_VT"
echo "========================================"
echo ""

# Create xinitrc for gar
XINITRC=$(mktemp)
cat > "$XINITRC" << EOF
#!/bin/sh
# Set up environment
export GAR_LOG=info

# Configure monitor layout: 1080p -> 1440p -> 4k (left to right)
# Wait a moment for X to fully initialize
sleep 0.5
xrandr \\
  --output DP-1 --mode 1920x1080 --pos 0x0 \\
  --output HDMI-1 --mode 2560x1440 --pos 1920x0 \\
  --output HDMI-0 --mode 3840x2160 --pos 4480x0

# Ensure gar config directory exists
mkdir -p ~/.config/gar

# Launch compositor before WM (for proper screen repainting)
# gar generates picom.conf on startup and signals picom to reload
if command -v picom > /dev/null 2>&1; then
    if [[ -f ~/.config/gar/picom.conf ]]; then
        picom -b --config ~/.config/gar/picom.conf &
    else
        # First run: start with GLX backend, gar will generate config and signal reload
        picom -b --backend glx &
    fi
    sleep 0.1
fi

# Start gar
exec $GAR_BIN
EOF
chmod +x "$XINITRC"

# Trap to clean up
cleanup() {
    rm -f "$XINITRC"
    echo ""
    echo "gar exited. Press Ctrl+Alt+F1 to return to SDDM/Hyprland."
}
trap cleanup EXIT

# Start X with gar
echo "Starting X server with gar..."
echo "Press Ctrl+C here or Super+Shift+Escape in gar to exit"
echo ""

# Use xinit directly - show all output for debugging
# -keeptty is required for systemd-logind integration
xinit "$XINITRC" -- "$DISPLAY_NUM" "vt$CURRENT_VT" -keeptty
