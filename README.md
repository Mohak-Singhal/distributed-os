# Personal Distributed Operating System (PDOS) — Runtime v0.1

A production-grade, secure, and cross-platform networking foundation designed to make all your devices behave as a single, unified computer.

This is a private repository designed with open-source quality standards.

## The Vision

Imagine if your macOS laptop, Android phone, Windows workstation, Linux server, and future browser agents worked together as a single operating system. Instead of talking to specific devices using platform-dependent protocols, the system treats every endpoint as a **Node** and advertises its **Capabilities** (e.g., Compute, Storage, AI Models, Camera, Remote Execution).

**Version 0.1** establishes the zero-trust networking foundation:
* **Node Identity:** Permanent cryptographic identifiers per device (Ed25519).
* **Discovery:** Real-time online/offline registration.
* **Message Routing:** A secure relay routing versioned packets.
* **Native Cross-Platform Execution:** A shared Rust runtime running natively on macOS (CLI) and Android (JNI Background Service).

```text
                PDOS Runtime

          ┌──────── Relay ────────┐
          │                       │
   Desktop Runtime         Android Runtime
          │                       │
          └──── Shared Rust Runtime ────┘
                     │
          Identity • Tasks • Search
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

## Technical Implementation

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

### 3. Capability Advertisement
Nodes advertise exactly what they are capable of. The `Capability` system allows future versions of the PDOS engine to route workloads automatically. 
When a node connects to the relay, it advertises capabilities such as:
```rust
pub enum Capability {
    Compute,
    FileStorage,
    Search,
    Docker,
    AiModel,
    Browser,
    Notifications,
    Camera,
    Microphone,
    Terminal,
    RemoteExecution,
}
```

### 4. Node Registry
The relay maintains an in-memory registry of currently connected nodes. Persistent node identity and pairing information are stored locally by each node (the relay is not a database). Each connected node record contains:
- **Node ID**
- **Name**
- **Platform**
- **Version**
- **Status**
- **Capabilities**
- **Last Heartbeat**

### 5. Universal Search
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
dos search capability=compute
```

**Returns:**
```text
Found 1 devices:
  [1.0] MacBook Air (mac - online) v0.1.0 ID: 732626c7...
      Capabilities: [compute, search]
```

### 6. Universal Task Manager
Everything in PDOS is represented as a Task. A central part of the V0.1 architecture is the fully extensible `dos-task-manager`.

Version 0.1 includes:
- `PingTask`
- `PairTask`
- `SearchTask`

Future versions will add:
- `FileTransferTask`
- `RemoteCommandTask`
- `DockerTask`
- `AIInferenceTask`

Without changing the underlying networking layer, new capabilities only require writing a new struct that implements the `Task` trait. 

**Execution Flow:**
```text
CLI / Agent
      ↓
TaskRequest
      ↓
Task Registry
      ↓
Task Dispatcher
      ↓
Executor
      ↓
TaskResult
      ↓
Relay
      ↓
Requester
```
When a `TaskRequest` arrives over the wire, the runtime dynamically resolves the task type against a `TaskRegistry`, instantiates the corresponding command, enqueues it to a `TaskDispatcher`, and automatically routes the result back to the caller. Both the CLI (which acts as a Client) and the Android/Desktop daemons (which act as Agents) rely on the exact same Task Manager abstractions.

### 7. Native Android Architecture
Instead of using Termux or emulation, the Android implementation is a true native app.
* **Rust Engine:** The shared `dos-runtime` is compiled to `arm64-v8a` and managed by a native Tokio multi-threaded runtime.
* **JNI Bridge:** A custom interface bridges the Rust engine and the Android JVM.
* **Foreground Service:** The node runs as a persistent Android Foreground Service, ensuring the OS doesn't kill the WebSocket connection when the screen is locked.
* **Reactive UI:** The Kotlin UI layer uses `StateFlow` to react in real-time to connection status and error events streamed from the Rust backend.

## Current Scope (v0.1)

**Implemented**
- [x] Node Identity
- [x] Relay
- [x] Pairing
- [x] Registry
- [x] Heartbeats
- [x] Universal Search (Nodes)
- [x] Universal Task Manager
- [x] Android Runtime
- [x] Desktop Runtime

**Not Yet Implemented**
- [ ] File Transfer
- [ ] Remote Terminal
- [ ] Distributed Compute
- [ ] Docker Nodes
- [ ] Browser Nodes
- [ ] AI Task Scheduling
- [ ] Synchronization

## Validation & Testing (End-to-End)

You can spin up the network locally to verify cross-device communication.

### 1. Start the Relay (Mac)
Run the central routing hub on your host machine:
```bash
cargo run -p dos-relay
```

### 2. Launch the Android Node
1. Connect your Android device via USB/ADB.
2. Ensure you are on the same local Wi-Fi network as your Mac.
3. Build and deploy the native app:
```bash
cd agents/android
cargo ndk -t arm64-v8a -o ./app/src/main/jniLibs build -p dos-android
./gradlew installDebug
```
4. Open the app on your phone and tap **Start Node**.

### 3. Inspect the Network (Mac CLI)
From your Mac, search the network for your Android phone and interact with it:
```bash
# See all connected devices (should list your Mac CLI and the Android phone)
cargo run -p dos-cli -- search ""
```
**Returns:**
```text
Found 1 devices:
  [1.0] Mohak's S23 (android - online) v0.1.0 ID: dd23f0d5-5c31-597f-9942-525e211c4bb9
      Capabilities: [compute, notifications, camera]
```

```bash
# Ping the Android phone to test round-trip latency
cargo run -p dos-cli -- ping <ANDROID_NODE_ID>
```
**Returns:**
```text
Reply from dd23f0d5-5c31-597f-9942-525e211c4bb9: time=82.1ms result={"message":"pong","success":true}
```

```bash
# Pair with the Android phone
cargo run -p dos-cli -- pair <ANDROID_NODE_ID>
```

## License
This is a private, proprietary software project. All rights reserved.
