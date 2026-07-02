# Release Notes: PDOS Runtime v0.2.0

We are excited to announce the release of **PDOS Runtime v0.2.0**. This milestone transitions the PDOS network from a pure discovery layer into a fully functional **Communication Layer**. 

Nodes can now securely execute tasks, transfer data, and interact across platforms utilizing the robust Task Manager architecture introduced in v0.1.

## New Features

### 1. Unified Terminal Execution
Execute shell commands securely on remote nodes.
- Built-in `TerminalProvider` handles sandboxing and OS-specific invocation (e.g., `Command` on Desktop, generic shell execution on Android).
- Captures `stdout` and `stderr` and streams the result back through the WebSocket envelope.
- Usage: `dos exec <node_id> <command> [args]`

### 2. Bidirectional File Transfer
Send and receive files directly between nodes over the existing WebSocket protocol.
- Introduces the `FileTask` which handles chunked base64 encoding and decoding.
- Utilizes the established `dos_protocol::Message::TaskRequest` payload structure, maintaining zero architectural deviation.
- Usage: `dos send-file <node_id> <local> <remote>` and `dos get-file <node_id> <remote> <local>`

### 3. Cross-Platform Notifications
Trigger native system notifications on remote devices silently and reliably.
- Implements `NotificationsProvider` across macOS (via `osascript`) and Android (via native `NotificationCompat.Builder` JNI bridge).
- Usage: `dos notify <node_id> <title> <body>`

### 4. Remote Clipboard Access
Read from and write to the system clipboard of remote nodes.
- Seamlessly synchronize clipboard data between Desktop and Android environments.
- Usage: `dos clipboard get <node_id>` and `dos clipboard set <node_id> <text>`

## Enhancements & Architecture Improvements

- **Capability Broadcasting:** Nodes now automatically harvest supported capabilities (e.g., `"terminal"`, `"clipboard"`) from their loaded Task Registry. These are embedded in the `HeartbeatPayload`, allowing the Relay to broadcast node features.
- **Enhanced Search:** The `dos search` CLI command now displays node capabilities dynamically.
- **Provider-Injection Formalization:** Fully stabilized the `dos_task_manager::providers` trait system, keeping core agent logic decoupled from underlying OS APIs.
- **Bidirectional Heartbeat Acknowledgement:** Added `HeartbeatAck` protocol frames. The relay now acknowledges client heartbeats, and agents monitor these to actively detect and drop half-open TCP connections.
- **Registry Connection ID Isolation:** Each socket connection is assigned a unique UUID in the relay registry, preventing overlapping connection handlers from removing newly established sessions during reconnect race conditions.

## Bug Fixes

- **Connection Stability:** Fixed a critical bug in `WsConnection::recv()` where WebSocket `Ping`/`Pong` control frames were misinterpreted as connection closures, prematurely dropping short-lived CLI tasks.
- **Agent Run Loop Retention:** Fixed a bug where normal socket close (`Ok(())`) broke the agent run loop and permanently stopped background connection threads. The agent now robustly retries every 5 seconds.
- **Android Service Thread Leak Prevention:** Overhauled `NodeService` to gracefully terminate any old native agent task thread before spawning a new one.
- **Protocol Validation:** Resolved missing fields (`capabilities`, `to`) in the protocol validation boundaries and unit test suites, achieving 100% test coverage.
- **Cleanup:** Resolved dead code and unused variable warnings in the relay node.

## Upgrading

This release is a direct extension of v0.1. The networking topology and port configuration (`7890`) remain identical. Ensure all network nodes (Relay, Desktop, Android, and CLI) are updated to v0.2.0 concurrently to prevent `ProtocolError::VersionMismatch` drops.
