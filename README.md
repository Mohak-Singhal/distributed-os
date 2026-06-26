# Personal Distributed Operating System (PDOS) — Network Core (v0.1)

A production-grade, secure, and cross-platform networking foundation designed to make all your devices behave as a single, unified computer.

This is a private repository designed with open-source quality standards in mind.

---

## 🌟 The Vision

Imagine if your macOS laptop, Android phone, Windows workstation, Linux server, and future browser agents worked together as a single operating system. Instead of talking to specific devices using platform-dependent protocols, the system treats every endpoint as a **Node** and advertises its **Capabilities** (e.g., Compute, Storage, AI Models, Camera, Remote Execution).

**Version 0.1** establishes the zero-trust networking foundation:
* **Node Identity:** Permanent cryptographic identifiers per device.
* **Discovery:** Real-time online/offline registration.
* **Message Routing:** A secure relay routing versioned packets.
* **Task Dispatched Queues:** The ability to route and execute commands on other devices (e.g. Ping).

---

## 📁 Repository Layout

The project uses a Cargo workspace to maintain clean boundaries between components:

```text
├── agents/
│   ├── desktop/          # macOS, Linux, and Windows background daemon
│   └── android/          # Android JNI library wrapper (cdylib)
├── cli/                  # Command-line controller (`dos` binary)
├── relay/                # Central WebSocket routing hub
├── platform/             # OS-specific hardware/system monitors
│   ├── mac/ | windows/ | linux/ | android/
├── crates/               # Core libraries
│   ├── core/             # Base models (Node, Capability, Task)
│   ├── crypto/           # Ed25519 signing and node verification
│   ├── protocol/         # Packet schemas, builders, and codecs
│   ├── networking/       # WebSocket connection abstraction
│   ├── storage/          # SQLite persistence layer (repositories)
│   ├── task_manager/     # Task queue and Ping implementation
│   ├── search/           # Registry node search
│   └── common/           # Error types, configs, and utilities
└── migrations/           # SQLite schema migrations
```

---

## 🛠️ Technical Architecture

### 1. Cryptographic Identity & Persistence
Every node generates an **Ed25519 keypair** on its first run. 
* The **Node ID** is a deterministic UUID v5 derived from the public key, ensuring it never changes.
* The private key is securely stored in a local SQLite settings repository (`dos.db`). 
* Path handling uses raw file connections (`SqliteConnectOptions`) to support directory paths containing spaces.

### 2. The Custom Wire Protocol (`dos-protocol`)
All WebSocket frames carry exactly one versioned `Envelope`:
```json
{
  "version": 1,
  "message_id": "550e8400-e29b-41d4-a716-446655440000",
  "type": "task_request",
  "from": "732626c7-40c3-53bd-9110-848e1c0d457b",
  "to": "3f9a1ce3-99e3-5d21-b366-4906c804b6c5",
  "task_id": "84aed54f-2742-47dc-954d-ff1430a83171",
  "task_type": "ping",
  "payload": {}
}
```
* **Version Control:** Nodes reject mismatched versions immediately.
* **Correlated Transports:** Every request-response pair uses `message_id` tracking.

### 3. Relay Routing Node
The `dos-relay` acts as a zero-state, high-performance router. It maintains a registry of connected clients in memory and routes packets to target nodes based on their destination ID (`to`). It performs strict heartbeat checks to prune offline clients.

---

## 🚀 Getting Started

### 📋 Prerequisites
* **Rust:** Install via `rustup` (1.75+ recommended).
* **SQLite:** Native SQLite development libraries (installed by default on macOS/Windows).

### 🖥️ Local Verification (loopback on Mac)

Open three terminal windows side-by-side:

#### 1. Start the Relay
```bash
cargo run -p dos-relay
```

#### 2. Start the Agent
```bash
cargo run -p dos-desktop
```
*Copy the `node_id` printed in the startup logs (e.g. `732626c7-40c3-53bd-9110-848e1c0d457b`).*

#### 3. Control via the CLI
Use the CLI client to search the network and dispatch commands:
```bash
# Search for online nodes
cargo run -p dos-cli -- search ""

# Ping the agent (measures round-trip latency)
cargo run -p dos-cli -- ping <NODE_ID>

# Pair with the agent
cargo run -p dos-cli -- pair <NODE_ID>
```

---

## 📱 Running on Android (via Termux)

1. Make sure your Mac has **Remote Login (SSH)** enabled in *System Settings > General > Sharing*.
2. Install **Termux** on your Android phone.
3. Open Termux and set up the Rust environment:
   ```bash
   pkg update -y && pkg install rust git -y
   ```
4. Clone the repository directly from your Mac's IP:
   ```bash
   git clone ssh://<username>@<mac-ip>/Users/mohaksinghal/Desktop/codeit/Device\ manager/distributed-os
   cd distributed-os
   ```
5. Point the client config to your Mac's relay server:
   ```bash
   echo 'relay_url = "ws://<mac-ip>:7890"' > dos-config.toml
   ```
6. Run the agent:
   ```bash
   cargo run -p dos-desktop
   ```
7. Go back to your Mac CLI and run the search command—your Android phone will show up immediately!

---

## 📦 Building Native Android Libraries (`cdylib`)

The `agents/android` crate compiles into a C-compatible library (`.so`) that can be loaded into an Android Studio Kotlin/Java app using JNI.

1. Install target toolchains:
   ```bash
   rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
   ```
2. Install `cargo-ndk`:
   ```bash
   cargo install cargo-ndk
   ```
3. Compile (requires Android NDK installed):
   ```bash
   cargo ndk -t arm64-v8a -o ./jniLibs build -p dos-android --release
   ```
This generates the shared library files containing the native JNI bridge `Java_com_dos_agent_Core_startAgent`.

---

## 🔒 License & Usage
This is a private, proprietary software project. All rights reserved. Do not distribute without explicit permission.
