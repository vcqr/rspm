#!/bin/bash
# Linux Service Installation Script for RSPM Daemon (rspmd)

set -e

RSPM_DIR="$HOME/.rspm"
SERVICE_FILE="rspmd.service"
INSTALL_SCRIPT="linux-install.sh"

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

# Copy service file
SERVICE_SRC="$(dirname "$0")/$SERVICE_FILE"
SERVICE_DST="$HOME/.config/systemd/user/$SERVICE_FILE"

if [ -f "$SERVICE_SRC" ]; then
    mkdir -p "$HOME/.config/systemd/user"
    cp "$SERVICE_SRC" "$SERVICE_DST"
    
    # Replace %u and %g with actual user
    sed -i "s/%u/$USER/g" "$SERVICE_DST"
    sed -i "s/%g/$(id -gn)/g" "$SERVICE_DST"
    
    echo "Service file installed to: $SERVICE_DST"
else
    echo "Warning: $SERVICE_FILE not found, creating default service file..."
    mkdir -p "$HOME/.config/systemd/user"
    
    cat > "$SERVICE_DST" << EOF
[Unit]
Description=RSPM Process Manager
Documentation=https://github.com/vcqr/rspm
After=network.target

[Service]
Type=simple
User=$USER
Group=$(id -gn)
WorkingDirectory=$RSPM_DIR
ExecStart=$RSPMD_PATH
Restart=on-failure
RestartSec=5
StandardOutput=append:$RSPM_DIR/logs/daemon-out.log
StandardError=append:$RSPM_DIR/logs/daemon-err.log
Environment="RSPM_HOME=$RSPM_DIR"
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=$RSPM_DIR

[Install]
WantedBy=default.target
EOF
fi

# Reload systemd
echo ""
echo "Reloading systemd daemon..."
systemctl --user daemon-reload

# Enable service
echo "Enabling rspmd service..."
systemctl --user enable rspmd

echo ""
echo "========================================"
echo "  Installation Complete!"
echo "========================================"
echo ""
echo "To start the service:"
echo "  systemctl --user start rspmd"
echo ""
echo "To stop the service:"
echo "  systemctl --user stop rspmd"
echo ""
echo "To check status:"
echo "  systemctl --user status rspmd"
echo ""
echo "To view logs:"
echo "  journalctl --user -u rspmd -f"
echo ""
echo "To uninstall:"
echo "  systemctl --user stop rspmd"
echo "  systemctl --user disable rspmd"
echo "  rm ~/.config/systemd/user/rspmd.service"
echo ""