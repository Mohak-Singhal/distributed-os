#!/bin/bash
set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}=== Starting PDOS Mac Hub ===${NC}"

echo "Building binaries..."
cargo build --bin dos-relay --bin dos-desktop --bin dos

echo "Stopping any existing background processes..."
pkill -f "dos-relay" || true
pkill -f "dos-desktop" || true
pkill -f "dos dashboard" || true
sleep 1

echo "Starting Relay..."
./target/debug/dos-relay > /tmp/pdos_relay.log 2>&1 &

echo "Starting Desktop Agent..."
./target/debug/dos-desktop > /tmp/pdos_desktop.log 2>&1 &

echo "Starting Control Hub Web UI..."
./target/debug/dos dashboard 8080 > /tmp/pdos_dashboard.log 2>&1 &

echo -e "\n${GREEN}✅ Success! All PDOS services are running in the background.${NC}"
echo -e "${YELLOW}--------------------------------------------------${NC}"
echo -e "1. Open the ${BLUE}PDOS Android App${NC} on your phone."
echo -e "2. Tap ${GREEN}'Start Node'${NC} to auto-discover the Mac."
echo -e "3. Once it says 'Connected', tap ${BLUE}'Load Control Hub Web UI'${NC}."
echo -e "${YELLOW}--------------------------------------------------${NC}"
echo -e "To stop the services later, run: ${GREEN}pkill -f dos-${NC}"
