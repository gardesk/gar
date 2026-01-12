#!/usr/bin/env bash
# Start gar window manager on a real X session
#
# PANIC MODE: Alt+Shift+Escape to exit gar immediately
# BACKUP: Ctrl+Alt+F2 to switch to TTY2 if gar freezes
#
# Usage:
#   ./start-gar.sh        # Start on next available display
#   ./start-gar.sh :2     # Start on specific display
#
# To return to Hyprland after exiting:
#   Just log out and log back in, or run: Hyprland

set -e

GAR_DIR="$(cd "$(dirname "$0")" && pwd)"
GAR_BIN="$GAR_DIR/target/release/gar"

# Check if gar is built
if [[ ! -x "$GAR_BIN" ]]; then
    echo "Error: gar not found at $GAR_BIN"
    echo "Run: nix-shell --run 'cargo build --release'"
    exit 1
fi

# Find display
DISPLAY_NUM="${1:-:1}"

echo "========================================"
echo "  Starting gar window manager"
echo "========================================"
echo ""
echo "  PANIC MODE: Alt+Shift+Escape"
echo "  TTY ESCAPE: Ctrl+Alt+F2"
echo ""
echo "  Display: $DISPLAY_NUM"
echo "========================================"
echo ""

# Create xinitrc for gar
XINITRC=$(mktemp)
cat > "$XINITRC" << EOF
#!/bin/sh
# Set up environment
export GAR_LOG=info

# Start gar
exec $GAR_BIN
EOF
chmod +x "$XINITRC"

# Trap to clean up
cleanup() {
    rm -f "$XINITRC"
    echo ""
    echo "gar exited. You can now return to your normal session."
}
trap cleanup EXIT

# Start X with gar
echo "Starting X server with gar..."
echo "Press Ctrl+C here or Alt+Shift+Escape in gar to exit"
echo ""

startx "$XINITRC" -- "$DISPLAY_NUM" vt$(tty | grep -o '[0-9]*' || echo 7)
