#!/bin/bash
# macOS Service Installation Script for RSPM Daemon (rspmd)

set -e

RSPM_DIR="$HOME/.rspm"
PLIST_FILE="com.rspm.daemon.plist"

echo "========================================"
echo "  RSPM Daemon Service Installation"
echo "========================================"
echo ""

# Detect rspmd path
RSPMD_PATH=""
if [ -f "$(dirname "$0")/rspmd" ]; then
    RSPMD_PATH="$(dirname "$0")/rspmd"
elif [ -f "$(dirname "$0")/target/release/rspmd" ]; then
    RSPMD_PATH="$(dirname "$0")/target/release/rspmd"
elif command -v rspmd &> /dev/null; then
    RSPMD_PATH=$(which rspmd)
else
    echo "Error: rspmd not found"
    echo "Please place this script next to rspmd or add rspmd to PATH"
    exit 1
fi

echo "RSPM Directory: $RSPM_DIR"
echo "RSPMD Executable: $RSPMD_PATH"
echo ""

# Create RSPM directories
mkdir -p "$RSPM_DIR"/{db,logs,pid,sock}

# Copy plist file
PLIST_SRC="$(dirname "$0")/$PLIST_FILE"
PLIST_DST="$HOME/Library/LaunchAgents/$PLIST_FILE"

if [ -f "$PLIST_SRC" ]; then
    cp "$PLIST_SRC" "$PLIST_DST"
    echo "Plist installed to: $PLIST_DST"
else
    echo "Warning: $PLIST_FILE not found, creating default plist..."
    mkdir -p "$HOME/Library/LaunchAgents"
    
    cat > "$PLIST_DST" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.rspm.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>$RSPMD_PATH</string>
    </array>
    <key>WorkingDirectory</key>
    <string>$RSPM_DIR</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>$RSPM_DIR/logs/daemon-out.log</string>
    <key>StandardErrorPath</key>
    <string>$RSPM_DIR/logs/daemon-err.log</string>
    <key>ProcessType</key>
    <string>Background</string>
    <key>ThrottleInterval</key>
    <integer>10</integer>
</dict>
</plist>
EOF
fi

# Fix path in plist
sed -i '' "s|~/.rspm|$RSPM_DIR|g" "$PLIST_DST"
sed -i '' "s|/usr/local/bin/rspmd|$RSPMD_PATH|g" "$PLIST_DST"

# Set permissions
chmod 644 "$PLIST_DST"
chown "$USER:staff" "$PLIST_DST"

# Load service
echo "Loading rspmd service..."
launchctl load "$PLIST_DST"

# Enable for login item
echo "Enabling rspmd to start at login..."
launchctl enable "user/$UID/com.rspm.daemon"

echo ""
echo "========================================"
echo "  Installation Complete!"
echo "========================================"
echo ""
echo "To start the service:"
echo "  launchctl start com.rspm.daemon"
echo ""
echo "To stop the service:"
echo "  launchctl stop com.rspm.daemon"
echo ""
echo "To check status:"
echo "  launchctl list | grep rspm"
echo ""
echo "To view logs:"
echo "  tail -f $RSPM_DIR/logs/daemon-out.log"
echo ""
echo "To uninstall:"
echo "  launchctl stop com.rspm.daemon"
echo "  launchctl unload $PLIST_DST"
echo "  rm $PLIST_DST"
echo ""