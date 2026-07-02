#!/bin/bash
killall dos-relay 2>/dev/null
nohup $HOME/Library/Application\ Support/PDOS/dos-relay > $HOME/.pdos/logs/relay.log 2>&1 &
exec $HOME/Library/Application\ Support/PDOS/dos dashboard 8080