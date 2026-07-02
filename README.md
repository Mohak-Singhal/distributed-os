# Personal Distributed Operating System (PDOS) — Runtime v0.3

**A production-grade, secure, cross-platform runtime for building a personal distributed operating system.**

This is a private repository designed with open-source quality standards.

## The Vision

Imagine if your macOS laptop, Android phone, Windows workstation, Linux server, and future browser agents worked together as a single operating system. Instead of talking to specific devices using platform-dependent protocols, the system treats every endpoint as a **Node** and advertises its **Capabilities** (e.g., Compute, Storage, AI Models, Camera, Remote Execution).

* **Version 0.1** established the zero-trust networking foundation: cryptographic Ed25519 identity, real-time node registry, WebSocket packet routing, and a shared native Rust runtime.
* **Version 0.2** establishes the **Secure Communication Layer**, allowing nodes to securely exchange system clipboard contents, trigger native system notifications, execute remote terminal commands, and transfer files.
* **Version 0.3** delivers the **Consumer Experience Layer**: a premium monochrome Control Hub UI, production-grade bidirectional file transfer (streaming binary, no encoding overhead), Android share sheet integration, per-transfer Accept/Decline consent, SHA-256 integrity verification, and a real-time activity log.

```text
                           PDOS Runtime v0.2
                 CLI
                  │
                  ▼
            Task Manager
                  │
                  ▼
              Relay Hub
        ┌─────────┴─────────┐
        ▼                   ▼
 Desktop Host         Android Host
        │                   │
        └──── Shared Rust Runtime ────┘
                                 │
              Clipboard • Notifications • Terminal • Files
```

## Repository Architecture

The project uses a Cargo workspace to maintain clean boundaries between components, sharing a core runtime engine across all platforms.

```text
├── agents/
│   ├── desktop/          # macOS, Linux, and Windows background daemon
│   └── android/          # Native Android App + Foreground Service + JNI bridge
├── cli/                  # Command-line controller (`dos` binary)
├── relay/                # Central WebSocket routing hub
├── crates/               # Core libraries
│   ├── common/           # Shared utilities and configurations
│   ├── core/             # Base models (Node, Capability, Task)
│   ├── crypto/           # Ed25519 signing and node verification
│   ├── discovery/        # Network discovery mechanisms
│   ├── heartbeat/        # Telemetry and heartbeat system
│   ├── networking/       # WebSocket connection management
│   ├── protocol/         # Packet schemas, builders, and codecs
│   ├── runtime/          # The shared agent engine (Tokio runtime)
│   ├── search/           # Structured query indexing
│   ├── storage/          # SQLite persistence layer
│   └── task_manager/     # Dynamic task dispatch subsystem
```

## Technical Implementation (v0.2)

Every action in PDOS is modeled as a `Task` dispatched dynamically across the network. The standard execution flow looks like this:

```text
Mac CLI
   ↓
TerminalTask
   ↓
Relay Hub
   ↓
Android Runtime
   ↓
Task Registry
   ↓
TerminalProvider
   ↓
OS Command
   ↓
stdout
   ↓
Relay Hub
   ↓
Mac CLI
```

Version 0.2 expands the core JNI and desktop providers:

### 1. Clipboard Synchronization
* **Desktop**: Leverages native cross-platform clipboard listeners through the `arboard` library, running inside asynchronous Tokio blocking threads to interface with OS window systems.
* **Android**: Uses JNI to access Android’s native `ClipboardManager` via a main thread loop handler callback.
* **Commands**:
  ```bash
  dos clipboard set <target_node_id> "Hello World"
  dos clipboard get <target_node_id>
  ```

### 2. Native System Notifications
* **macOS**: Leverages AppleScript (`osascript`) headlessly to spawn standard system banners.
* **Android**: Interfaces with Android’s `NotificationCompat` through the JNI host interface, ensuring alerts display even when the application is minimized or backgrounded.
* **Commands**:
  ```bash
  dos notify <target_node_id> "Subject Line" "Message body text"
  ```

### 3. Remote Terminal Execution
* **Execution**: Commands are executed as child processes under the permissions of the running agent. Version 0.2 is intended for trusted, paired devices. Future versions may introduce command policies and stronger execution isolation.
* **Commands**:
  ```bash
  dos exec <target_node_id> "ls -la"
  dos exec <target_node_id> "uname -a"
  ```

### 4. Base64 Chunked File Transfer
* **Relay-mediated file transfer**: Files are chunked, base64-encoded, and transferred through the existing WebSocket task protocol. Future versions will support direct peer-to-peer transfers for improved throughput.
* **Commands**:
  ```bash
  dos send-file <target_node_id> ./local_file.txt /remote/path/file.txt
  dos get-file <target_node_id> /remote/path/file.txt ./local_file.txt
  ```

### 5. Resilient Connection Infrastructure
* **Bidirectional Heartbeat Acknowledgement**: The relay replies to agent heartbeats with a `HeartbeatAck` frame. If the agent does not receive an acknowledgement within 35 seconds, it automatically assumes the TCP socket is half-open (dead) and triggers a clean reconnection.
* **Registry Isolation**: Uses unique connection IDs (UUIDs) for each WebSocket session, preventing overlapping handler tasks from prematurely removing new connections from the relay registry (preventing the `target_offline` error during quick reconnection cycles).
* **Automatic Reconnection**: Overhauled the native Rust agent run loop to prevent thread exit on standard connection closures. The agent automatically retries connecting to the relay URL every 5 seconds.
* **Android Service Lifecycle Stability**: Overhauled the Android foreground service runtime to cleanly terminate previous threads and free system resources before spawning new agents.

---

## Technical Foundation (v0.1)

### 1. Cryptographic Identity & Persistence
Every node generates an **Ed25519 keypair** on its first run. 
* The **Node ID** is a deterministic UUID v5 derived from the public key, ensuring it never changes.
* The private key is securely stored in a local SQLite database (`dos.db`). 
* On Android, this database is safely sandboxed inside the app's internal private storage directory (`filesDir`), preventing data loss or unauthorized access.

### 2. Node Registration & Network Flow
When a node starts, it follows a deterministic registration flow. The Desktop and Android agents behave identically:
```text
Agent Runtime
      ↓
Generate Identity (Ed25519)
      ↓
Load Local State
      ↓
Connect to Relay
      ↓
Authenticate
      ↓
Register Node & Advertise Capabilities
      ↓
Start Heartbeat Loop
      ↓
Node becomes Searchable
      ↓
Accept & Execute Tasks
```

### 3. Universal Search
The Universal Search subsystem provides a common interface for discovering nodes across the distributed network. 
*Universal Search is intentionally limited to node discovery in Version 0.1. Future versions will extend the same interface to files, containers, browser tabs, AI models, and other distributed resources.*

Supported search fields:
- Node Name
- Node ID
- Platform
- Online Status
- Capabilities
- Version

**Examples:**
```bash
dos search mac
dos search android
dos search online
dos search capability=clipboard
```

**Returns:**
```text
Found 1 devices:
  [1.0] Pixel 8 (android - online) v0.1.0 ID: dd23f0d5...
      Capabilities: [notifications, clipboard, terminal, file_transfer]
```

---

## Current Scope

**Infrastructure**
- [x] Relay
- [x] Registry
- [x] Heartbeats
- [x] Search

**Security**
- [x] Ed25519 Identity
- [x] Pairing
- [x] Authentication

**Communication**
- [x] Clipboard
- [x] Notifications
- [x] Terminal
- [x] File Transfer

**Host Support**
✓ macOS
✓ Android
• Linux (In Progress)
• Windows (In Progress)

---

## Roadmap

**v0.3**
- Remote Desktop
- Keyboard
- Mouse

**v0.4**
- Docker Nodes
- Browser Nodes
- Distributed Compute

**v1.0**
- AI Task Scheduling
- Unified Personal Distributed OS

---

## Validation & Testing (End-to-End)

You can spin up the network locally or across physical networks to verify cross-device communication.

### 1. Local loopback verification (macOS)
To run the automated loopback integration suite:
```bash
./scripts/test_v0.2.sh
```

### 2. Cross-Device Verification (macOS ↔ Android)

#### A. Start the Relay & Mac Agent (Mac)
Run the routing hub and local desktop agent on your host machine:
```bash
# Terminal 1 (Mac)
cargo run --bin dos-relay

# Terminal 2 (Mac)
cargo run --bin dos-desktop
```

#### B. Build & Deploy the Android Agent
1. Connect your Android device via USB/ADB.
2. Ensure you are on the same local Wi-Fi network.
3. Build the native JNI libraries and install the app package:
```bash
cd agents/android
cargo ndk -t arm64-v8a -o ./app/src/main/jniLibs build
./gradlew installDebug
```
4. Launch the **PDOS Node** app on your phone, verify your Mac's IP (e.g. `ws://192.168.1.3:7890`), and tap **Start Node**.

#### C. Control your Android Phone from macOS CLI
Open a terminal on your Mac and run the controller tasks:
```bash
# Discover devices (look for the Android platform node ID)
cargo run --bin dos -- search ""

# Send a native notification to Android
cargo run --bin dos -- notify <ANDROID_NODE_ID> "Mac Command" "Hello Android OS!"

# Set Android's system clipboard
cargo run --bin dos -- clipboard set <ANDROID_NODE_ID> "Sent from Mac!"

# Run a shell command on Android
cargo run --bin dos -- exec <ANDROID_NODE_ID> "id"

# Send a file to Android's internal app files storage
cargo run --bin dos -- send-file <ANDROID_NODE_ID> /tmp/mac_file.txt /data/data/com.dos.agent/files/transferred.txt
```

## License
This is a private, proprietary software project. All rights reserved.
