#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== Starting PDOS v0.2 Verification Suite ===${NC}"

# Ensure binaries are built
echo "Building binaries..."
cargo build --bin dos-relay --bin dos-desktop --bin dos

# Clean up any lingering agents/relays
pkill -f "dos-relay" || true
pkill -f "dos-desktop" || true
sleep 1

# Start the Relay in the background
echo "Starting relay..."
./target/debug/dos-relay > /tmp/dos-relay.log 2>&1 &
RELAY_PID=$!
sleep 1

# Start the Desktop Agent in the background
echo "Starting desktop agent..."
./target/debug/dos-desktop > /tmp/dos-desktop.log 2>&1 &
DESKTOP_PID=$!

# Give them a moment to connect and settle
sleep 2

# Verify they are running
if ! ps -p $RELAY_PID > /dev/null; then
    echo -e "${RED}Relay failed to start. Check /tmp/dos-relay.log${NC}"
    exit 1
fi
if ! ps -p $DESKTOP_PID > /dev/null; then
    echo -e "${RED}Desktop agent failed to start. Check /tmp/dos-desktop.log${NC}"
    exit 1
fi

# Retrieve the Desktop Agent's Node ID (filtering out the CLI node by looking for 'clipboard' capability)
echo "Searching for desktop node..."
NODE_ID=$(./target/debug/dos search "" 2>/dev/null | grep -B 1 "clipboard" | grep "ID:" | head -n 1 | awk '{print $NF}')

if [ -z "$NODE_ID" ]; then
    echo -e "${RED}Failed to find Desktop Agent via Search API.${NC}"
    kill $RELAY_PID $DESKTOP_PID || true
    exit 1
fi

echo -e "${GREEN}Found Desktop Node ID: $NODE_ID${NC}"

PASS=0
FAIL=0

run_assertion() {
    local name="$1"
    local actual="$2"
    local expected="$3"
    
    if echo "$actual" | grep -q "$expected"; then
        echo -e "  ✅ ${GREEN}PASS:${NC} $name"
        PASS=$((PASS + 1))
    else
        echo -e "  ❌ ${RED}FAIL:${NC} $name"
        echo "     Expected match: $expected"
        echo "     Actual Output:  $actual"
        FAIL=$((FAIL + 1))
    fi
}

echo -e "\n${GREEN}--- Running Tests ---${NC}"

# 1. Clipboard Set
echo "Testing Clipboard Set..."
OUT=$(./target/debug/dos clipboard set "$NODE_ID" "PDOS-INTEGRATION-TEST-VALUE" 2>&1)
run_assertion "Clipboard Set Command" "$OUT" "Clipboard set successfully"

# 2. Clipboard Get
echo "Testing Clipboard Get..."
OUT=$(./target/debug/dos clipboard get "$NODE_ID" 2>&1)
run_assertion "Clipboard Get Command" "$OUT" "PDOS-INTEGRATION-TEST-VALUE"

# 3. Notification
echo "Testing Notification..."
OUT=$(./target/debug/dos notify "$NODE_ID" "Integration Test" "PDOS is working" 2>&1)
run_assertion "Notification Trigger" "$OUT" "Notification sent"

# 4. Terminal execution (pwd)
echo "Testing Terminal Exec (pwd)..."
OUT=$(./target/debug/dos exec "$NODE_ID" pwd 2>&1)
run_assertion "Terminal Exec pwd" "$OUT" "distributed-os"

# 5. Terminal execution (echo)
echo "Testing Terminal Exec (echo)..."
OUT=$(./target/debug/dos exec "$NODE_ID" echo "verification-payload" 2>&1)
run_assertion "Terminal Exec echo" "$OUT" "verification-payload"

# 6. File Transfer (Send)
echo "Testing File Transfer (Send)..."
echo "PDOS-FILE-TRANSFER-DATA" > /tmp/pdos_send_test.txt
OUT=$(./target/debug/dos send-file "$NODE_ID" /tmp/pdos_send_test.txt /tmp/pdos_recv_test.txt 2>&1)
run_assertion "File Send" "$OUT" "written"

# 7. File Transfer (Get & verify integrity)
echo "Testing File Transfer (Get)..."
OUT=$(./target/debug/dos get-file "$NODE_ID" /tmp/pdos_recv_test.txt /tmp/pdos_readback.txt 2>&1)
run_assertion "File Get" "$OUT" "success"

if [ -f /tmp/pdos_readback.txt ]; then
    INTEGRITY=$(cat /tmp/pdos_readback.txt)
    run_assertion "File Content Integrity" "$INTEGRITY" "PDOS-FILE-TRANSFER-DATA"
else
    run_assertion "File Content Integrity" "File not found" "PDOS-FILE-TRANSFER-DATA"
fi

# Clean up background tasks
kill $RELAY_PID $DESKTOP_PID || true
wait $RELAY_PID $DESKTOP_PID 2>/dev/null || true

# Remove temp files
rm -f /tmp/pdos_send_test.txt /tmp/pdos_recv_test.txt /tmp/pdos_readback.txt

echo -e "\n${GREEN}=== Verification Finished ===${NC}"
echo -e "${GREEN}Passed: $PASS${NC} | ${RED}Failed: $FAIL${NC}"

if [ $FAIL -ne 0 ]; then
    exit 1
fi
exit 0
