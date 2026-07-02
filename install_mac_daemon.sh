#!/bin/bash

# Build the release binary
echo "Compiling dos-cli for release..."
cargo build --release -p dos-cli
cargo build --release --bin dos-relay

# Get absolute path to binary
BIN_PATH="$(pwd)/target/release/dos"
if [ ! -f "$BIN_PATH" ]; then
    echo "Error: Binary not found at $BIN_PATH"
    exit 1
fi

PLIST_PATH="$HOME/Library/LaunchAgents/com.pdos.daemon.plist"
LOG_DIR="$HOME/.pdos/logs"
mkdir -p "$LOG_DIR"

# Create launchd plist
cat <<EOF > "$PLIST_PATH"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.pdos.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/mohaksinghal/Desktop/codeit/Device manager/distributed-os/start_pdos_services.sh</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$LOG_DIR/pdos_daemon.log</string>
    <key>StandardErrorPath</key>
    <string>$LOG_DIR/pdos_daemon.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
</dict>
</plist>
EOF

echo "Created plist at $PLIST_PATH"

# Unload if it already exists, then load it
cp target/release/dos "$HOME/Library/Application Support/PDOS/dos"
cp target/release/dos-relay "$HOME/Library/Application Support/PDOS/dos-relay"

launchctl unload "$PLIST_PATH" 2>/dev/null
launchctl load "$PLIST_PATH"

echo "PDOS Daemon successfully loaded into launchd!"
echo "It is now running silently in the background and will start on boot."
echo "Logs are available at $LOG_DIR"
