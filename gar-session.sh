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
