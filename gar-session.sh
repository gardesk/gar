#!/bin/bash
# gar session wrapper - sets up environment before starting gar

# Optional: configure monitor layout
# Uncomment and customize for your setup:
# xrandr --output eDP-1 --mode 2880x1800 --pos 0x0
# xrandr --output DP-1 --mode 1920x1080 --pos 0x0 \
#   --output HDMI-1 --mode 2560x1440 --pos 1920x0

# Launch compositor before WM (for proper screen repainting)
# --backend glx is required for picom v12+ (no longer has a default)
if command -v picom &> /dev/null; then
    picom -b --backend glx --use-ewmh-active-win &
    sleep 0.1
fi

# Set log level
export GAR_LOG=info

# Launch polybar after gar starts (needs i3 IPC socket)
(sleep 0.5 && ~/.config/polybar/launch.sh) &

# Start gar
exec /home/mfwolffe/GithubOrgs/tenseleyFlow/gar/target/release/gar
