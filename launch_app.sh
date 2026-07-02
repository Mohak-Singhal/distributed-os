#!/bin/bash
set -e
cd "$(dirname "$0")"

pkill -f "dos-relay" || true
pkill -f "dos-desktop" || true
pkill -f "dos dashboard" || true
sleep 0.5

./target/debug/dos-relay > /tmp/pdos_relay.log 2>&1 &
./target/debug/dos-desktop > /tmp/pdos_desktop.log 2>&1 &
./target/debug/dos dashboard 8080 > /tmp/pdos_dashboard.log 2>&1 &
