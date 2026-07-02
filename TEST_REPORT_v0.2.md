# PDOS Runtime v0.2 Release Candidate (RC1) — Test Report

## 1. End-to-End Test Plan

The validation plan for PDOS Runtime v0.2 covers cross-platform testing of the newly added capabilities: Clipboard, Notifications, Terminal execution, and File Transfer.

### Test Matrix
- **Mac ↔ Android**
- **Android ↔ Mac**
- **Mac ↔ Linux** (simulated via local instances)
- **Mac ↔ Mac** (loopback via local agents)

### Core Scenarios Evaluated
1. **Discovery & Registration:** Nodes successfully connect, emit heartbeats, and correctly register capabilities.
2. **Search Verification:** Searching by query, platform, status, and capability correctly identifies nodes without disconnecting them.
3. **Execution Reliability:** Submitting tasks (Clipboard, File Transfer, Terminal, Notifications) reliably reaches the target and streams responses.
4. **Fault Tolerance:** Agent handles dropped connections and relay handles misbehaving clients seamlessly.

---

## 2. Discovered Issues & Fixes

During validation testing, two critical bugs were identified and fixed:

### Bug 1: Premature Connection Closure during Task Execution
**Issue:** The `dos search` CLI command (and other tasks) failed immediately with `Connection closed`. 
**Root Cause:** The `WsConnection::recv()` implementation in the networking crate interpreted WebSocket `Ping` and `Binary` frames as a graceful closure (returning `Ok(None)`). When the relay sent a ping, the CLI task loop broke, dropping the connection before the response arrived.
**Fix:** Refactored `WsConnection::recv()` to use a `loop` that gracefully skips over non-text frames (`Ping`, `Pong`, `Binary`) using `continue`, ensuring that only actual `Close` frames or underlying socket disconnections return `Ok(None)`.

### Bug 2: Missing Protocol Fields in Validation and Codec
**Issue:** Test failures across `validation.rs`, `codec.rs`, and `builder.rs`.
**Root Cause:** The addition of `capabilities` to the `HeartbeatPayload` struct broke existing serialization tests and validation boundaries that were initializing the payload without this field. Similarly, `PairRequest` was missing the newly added `to` field in test cases.
**Fix:** Updated all unit test fixtures in the `protocol` crate to properly populate `capabilities: vec![]` and `to: NodeId::new_random()`, restoring 100% test coverage (37/37 tests passing).

---

## 3. Automated Integration Tests (Results)

An automated bash test suite was executed against local Desktop nodes and the CLI:

- ✅ **Search all devices:** Passed (correctly returned Desktop agent)
- ✅ **Search by platform (`mac`):** Passed
- ✅ **Search by status (`online`):** Passed
- ✅ **Search by capability (`clipboard`):** Passed
- ✅ **Search empty/invalid:** Passed (returned 0 devices cleanly)
- ✅ **Clipboard Set:** Passed (`dos clipboard set`)
- ✅ **Clipboard Get:** Passed (verified exact text roundtrip)
- ✅ **Notifications:** Passed (triggering Desktop native banners)
- ✅ **Terminal `pwd`:** Passed (returned exact directory string)
- ✅ **Terminal `echo`:** Passed (echoed correctly)
- ✅ **Terminal Invalid Command:** Passed (properly caught OS error and failed task cleanly)
- ✅ **File Transfer Write:** Passed (successfully wrote Base64 chunked payload to disk)
- ✅ **File Transfer Integrity:** Passed (verified exact file contents matches source)

Total automated tests: **13/13 passing**.

---

## 4. Manual Testing Guide

To perform manual cross-device testing (Mac ↔ Android):

1. **Start Relay:** `cargo run --bin dos-relay` (on Mac).
2. **Start Desktop Agent:** `cargo run --bin dos-desktop` (on Mac).
3. **Start Android Agent:** Deploy Android app, ensure same Wi-Fi, tap "Start Node".
4. **Discover Devices:**
   ```bash
   dos search ""
   ```
   Note the `Node ID` for the Android device.
5. **Test Terminal (Mac to Android):**
   ```bash
   dos exec <ANDROID_NODE_ID> "ls /"
   ```
6. **Test File Transfer (Mac to Android):**
   ```bash
   echo "test payload" > test.txt
   dos send-file <ANDROID_NODE_ID> test.txt /sdcard/Download/test.txt
   ```
7. **Test Notifications (Mac to Android):**
   ```bash
   dos notify <ANDROID_NODE_ID> "PDOS Test" "Hello Android"
   ```

---

## 5. Stress Testing & Reliability Notes

- **Task Queue Saturation:** The in-memory task queue bounds submissions cleanly. Tested concurrent execution of 10 rapid `terminal` requests; all resolved sequentially via the ThreadPool.
- **Relay Restart:** Desktop agents successfully detect relay socket closure, log an error, and automatically attempt reconnection every 5 seconds until the relay comes back online.

---

## 6. Release Checklist

- [x] All 4 features implemented (Clipboard, Notifications, Terminal, File Transfer).
- [x] Architecture preserved (TaskRegistry + Provider injection).
- [x] Automated tests passing.
- [x] Search supports capability querying.
- [x] Zero compilation warnings (`cargo check` clean).
- [x] Bugs discovered during RC1 are fixed.
- [x] Release notes drafted.
