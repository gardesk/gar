#!/bin/bash
# gar session wrapper - sets up monitors before starting gar

# Configure monitor layout: 1080p -> 1440p -> 4k (left to right)
# Output names: DP-1 (1080p), HDMI-1 (1440p), HDMI-0 (4k)
xrandr \
  --output DP-1 --mode 1920x1080 --pos 0x0 \
  --output HDMI-1 --mode 2560x1440 --pos 1920x0 \
  --output HDMI-0 --mode 3840x2160 --pos 4480x0

# Launch compositor before WM (for proper screen repainting)
# --use-ewmh-active-win uses _NET_ACTIVE_WINDOW for focus detection
if command -v picom &> /dev/null; then
    picom -b --use-ewmh-active-win &
    sleep 0.1  # Brief pause to let compositor initialize
fi

# Set log level
export GAR_LOG=info

# Start gar
exec /home/mfwolffe/GithubOrgs/tenseleyFlow/gar/target/release/gar
