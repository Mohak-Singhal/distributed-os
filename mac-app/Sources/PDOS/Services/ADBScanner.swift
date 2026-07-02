import Foundation

struct ADBDevice: Identifiable, Hashable, Equatable {
    let id: String
    let serial: String
    let state: String
    let model: String
    let marketName: String
    var batteryLevel: Int?
    var cpuPercent: Double?
    var ramUsed: Int64?
    var ramTotal: Int64?
    var storageUsed: Int64?
    var storageTotal: Int64?
    var networkRxSpeed: Double?
    var networkTxSpeed: Double?
    var isWireless: Bool { serial.contains(":") || serial.contains(".") }
    var displayName: String {
        if !marketName.isEmpty { return marketName }
        return model.isEmpty ? serial : model
    }
}

@MainActor
class ADBScanner: ObservableObject {
    static let shared = ADBScanner()

    @Published var devices: [ADBDevice] = []
    @Published var isAvailable = false
    @Published var scanError: String?

    private var pollTimer: Timer?
    private var analyticsTimer: Timer?
    private var didCheckAvailability = false
    private var prevNetBytes: [String: (rx: Int64, tx: Int64)] = [:]
    private var lastAnalyticsTime: Date?

    private init() {}

    func ensureAvailable() {
        if didCheckAvailability { return }
        didCheckAvailability = true

        let testResult = Shell.run("adb --version 2>/dev/null | head -1")
        if !testResult.isEmpty && testResult.contains("Android Debug Bridge") {
            isAvailable = true
            scanError = nil
            return
        }

        for path in adbPaths {
            if FileManager.default.isExecutableFile(atPath: path) {
                let test = Shell.run("\(path) --version 2>/dev/null | head -1")
                if test.contains("Android Debug Bridge") {
                    isAvailable = true
                    scanError = nil
                    return
                }
            }
        }

        isAvailable = false
        scanError = "ADB not found. Install Android Platform Tools."
    }

    private let adbPaths = [
        "/opt/homebrew/bin/adb",
        "/usr/local/bin/adb",
        "/usr/bin/adb",
        "\(NSHomeDirectory())/Library/Android/sdk/platform-tools/adb",
        "\(NSHomeDirectory())/.android/platform-tools/adb",
    ]

    func startScanning() {
        ensureAvailable()
        stopScanning()
        guard isAvailable else { return }
        scanOnce()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.scanOnce()
            }
        }
        startAnalytics()
    }

    func stopScanning() {
        pollTimer?.invalidate()
        pollTimer = nil
        stopAnalytics()
    }

    func scanOnce() {
        guard isAvailable else { return }
        let detected = scanDevices()
        devices = detected
    }

    private func scanDevices() -> [ADBDevice] {
        let result = Shell.adb("devices -l")
        guard !result.isEmpty else { return [] }

        var detected: [ADBDevice] = []
        let lines = result.components(separatedBy: .newlines)

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty, !trimmed.hasPrefix("List of"), !trimmed.hasPrefix("*") else { continue }

            let parts = trimmed.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
            guard parts.count >= 2 else { continue }
            let serial = String(parts[0])
            let info = String(parts[1])

            let state: String
            if info.hasPrefix("device") || info.contains("device ") {
                state = "device"
            } else if info.hasPrefix("offline") {
                state = "offline"
            } else if info.hasPrefix("unauthorized") {
                state = "unauthorized"
            } else {
                state = "unknown"
            }

            var model = ""
            var marketName = serial

            for pair in info.components(separatedBy: " ") {
                let kv = pair.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: true)
                if kv.count == 2 {
                    let key = kv[0].trimmingCharacters(in: .whitespaces)
                    let val = kv[1].trimmingCharacters(in: .whitespaces)
                    if key == "model" {
                        model = val.replacingOccurrences(of: "_", with: " ")
                    }
                }
            }

            if state == "device" {
                let deviceName = Shell.adb("-s \(serial) shell settings get global device_name 2>/dev/null").trimmingCharacters(in: .whitespacesAndNewlines)
                if !deviceName.isEmpty && deviceName != "null" {
                    marketName = deviceName
                } else {
                    let modelName = Shell.adb("-s \(serial) shell getprop ro.product.model 2>/dev/null").trimmingCharacters(in: .whitespacesAndNewlines)
                    if !modelName.isEmpty && modelName != "unknown" {
                        marketName = modelName
                    }
                    marketName = marketName.replacingOccurrences(of: "_", with: " ")
                }

                let batteryOutput = Shell.adb("-s \(serial) shell dumpsys battery 2>/dev/null")
                var batteryLevel: Int?
                for bLine in batteryOutput.components(separatedBy: .newlines) {
                    let bTrimmed = bLine.trimmingCharacters(in: .whitespaces)
                    if bTrimmed.hasPrefix("level:") {
                        let levelStr = bTrimmed.replacingOccurrences(of: "level:", with: "").trimmingCharacters(in: .whitespaces)
                        batteryLevel = Int(levelStr)
                    }
                }

                detected.append(ADBDevice(
                    id: serial,
                    serial: serial,
                    state: state,
                    model: model,
                    marketName: marketName,
                    batteryLevel: batteryLevel
                ))
            } else {
                detected.append(ADBDevice(
                    id: serial,
                    serial: serial,
                    state: state,
                    model: model,
                    marketName: marketName
                ))
            }
        }

        return detected
    }

    func refreshAnalytics() {
        guard isAvailable else { return }
        let now = Date()
        let dt = lastAnalyticsTime.map { now.timeIntervalSince($0) } ?? 10.0
        lastAnalyticsTime = now

        for i in devices.indices {
            let serial = devices[i].serial
            guard devices[i].state == "device" else { continue }

            let raw = Shell.adb("-s \(serial) shell \"top -bn1 2>/dev/null | head -5; echo '==MEM=='; cat /proc/meminfo 2>/dev/null | grep -E 'MemTotal|MemFree|MemAvailable'; echo '==DISK=='; df /data 2>/dev/null | tail -1; echo '==NET=='; cat /proc/net/dev 2>/dev/null | grep -E 'swlan|wlan|rndis|eth|ccmni'\"")
            let sections = raw.components(separatedBy: "==MEM==")
            guard sections.count >= 2 else { continue }
            let cpuBlock = sections[0]
            let rest = sections[1]
            let parts2 = rest.components(separatedBy: "==DISK==")
            guard parts2.count >= 2 else { continue }
            let memBlock = parts2[0]
            let rest2 = parts2[1]
            let parts3 = rest2.components(separatedBy: "==NET==")
            let diskBlock = parts3[0]
            let netBlock = parts3.count >= 2 ? parts3[1] : ""

            devices[i].cpuPercent = parseCPU(cpuBlock)
            let (ramU, ramT) = parseRAM(memBlock)
            devices[i].ramUsed = ramU
            devices[i].ramTotal = ramT
            let (storU, storT) = parseDisk(diskBlock)
            devices[i].storageUsed = storU
            devices[i].storageTotal = storT
            let (rxBytes, txBytes) = parseNet(netBlock)
            if let prev = prevNetBytes[serial], dt > 0 {
                devices[i].networkRxSpeed = Double(rxBytes - prev.rx) / dt
                devices[i].networkTxSpeed = Double(txBytes - prev.tx) / dt
            }
            prevNetBytes[serial] = (rxBytes, txBytes)
        }
    }

    private func parseCPU(_ block: String) -> Double? {
        // Look for "%Cpu(s):  usr/us/sy/id/..." line
        for line in block.components(separatedBy: .newlines) {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.contains("%Cpu") || t.contains("%CPU") {
                // Parse: "%Cpu(s):  5.2 us,  1.1 sy,  0.0 ni, 93.7 id, ..."
                let parts = t.replacingOccurrences(of: "%Cpu(s):", with: "")
                    .replacingOccurrences(of: "%Cpu", with: "")
                    .replacingOccurrences(of: "(s):", with: "")
                    .trimmingCharacters(in: .whitespaces)
                let tokens = parts.split(separator: " ")
                var user: Double = 0, sys: Double = 0, nice: Double = 0
                for (idx, token) in tokens.enumerated() {
                    if token == "us," || token == "us" { user = Double(tokens[safe: idx > 0 ? idx - 1 : 0].map(String.init) ?? "0") ?? 0 }
                    if token == "sy," || token == "sy" { sys = Double(tokens[safe: idx > 0 ? idx - 1 : 0].map(String.init) ?? "0") ?? 0 }
                    if token == "ni," || token == "ni" { nice = Double(tokens[safe: idx > 0 ? idx - 1 : 0].map(String.init) ?? "0") ?? 0 }
                }
                return user + sys + nice
            }
        }
        return nil
    }

    private func parseRAM(_ block: String) -> (Int64?, Int64?) {
        var total: Int64?, available: Int64?
        for line in block.components(separatedBy: .newlines) {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.hasPrefix("MemTotal:") {
                let val = t.replacingOccurrences(of: "MemTotal:", with: "").trimmingCharacters(in: .whitespaces)
                let numStr = val.split(separator: " ").first.map(String.init) ?? val
                total = Int64(numStr).map { $0 / 1024 } // kB → MB
            }
            if t.hasPrefix("MemAvailable:") {
                let val = t.replacingOccurrences(of: "MemAvailable:", with: "").trimmingCharacters(in: .whitespaces)
                let numStr = val.split(separator: " ").first.map(String.init) ?? val
                available = Int64(numStr).map { $0 / 1024 } // kB → MB
            }
        }
        if let t = total, let a = available {
            return (t - a, t)
        }
        return (nil, total)
    }

    private func parseDisk(_ block: String) -> (Int64?, Int64?) {
        let line = block.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !line.isEmpty else { return (nil, nil) }
        // Format: "Filesystem     1K-blocks    Used Available Use% Mounted on"
        //         "/dev/block/... 123456789 12345678 123456789 50% /data"
        let parts = line.split(separator: " ", omittingEmptySubsequences: true)
        guard parts.count >= 4 else { return (nil, nil) }
        let totalStr = String(parts[parts.count - 4])
        let usedStr = String(parts[parts.count - 3])
        guard let total = Int64(totalStr), let used = Int64(usedStr) else { return (nil, nil) }
        return (used / 1024, total / 1024) // kB → MB
    }

    private func parseNet(_ block: String) -> (Int64, Int64) {
        var rx: Int64 = 0, tx: Int64 = 0
        for line in block.components(separatedBy: .newlines) {
            let t = line.trimmingCharacters(in: .whitespaces)
            guard !t.contains("Inter-|") else { continue }
            // Format: "swlan0: 123456 789012 345 0 0 0 0 0 654321 987654 ..."
            if let colonIdx = t.firstIndex(of: ":") {
                let after = t[colonIdx...].dropFirst().trimmingCharacters(in: .whitespaces)
                let nums = after.split(separator: " ", omittingEmptySubsequences: true)
                if nums.count >= 10 {
                    rx += Int64(String(nums[0])) ?? 0
                    tx += Int64(String(nums[8])) ?? 0
                }
            }
        }
        return (rx, tx)
    }

    private func startAnalytics() {
        stopAnalytics()
        refreshAnalytics()
        analyticsTimer = Timer.scheduledTimer(withTimeInterval: 10.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.refreshAnalytics()
            }
        }
    }

    private func stopAnalytics() {
        analyticsTimer?.invalidate()
        analyticsTimer = nil
    }

    deinit {
        pollTimer?.invalidate()
        pollTimer = nil
        analyticsTimer?.invalidate()
    }
}

// MARK: - Wireless ADB

extension ADBScanner {
    /// Get the phone's IP address on its current WiFi/hotspot network
    nonisolated func getPhoneIP(serial: String) -> String? {
        // Check all interfaces: common WiFi names across devices
        let result = Shell.adb("-s \(serial) shell ip -f inet addr show 2>/dev/null | grep -v '127.0.0.1' | grep 'inet '")
        for line in result.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { continue }
            // Extract IP from format: "inet 10.75.146.108/24 brd ... scope global swlan0"
            let parts = trimmed.split(separator: " ")
            if parts.count >= 2 {
                let ipWithPrefix = String(parts[1])
                let ip = ipWithPrefix.split(separator: "/").first.map(String.init) ?? ipWithPrefix
                if !ip.isEmpty { return ip }
            }
        }
        return nil
    }

    /// Switch a USB-connected device to wireless (TCP/IP) mode
    nonisolated func switchToWireless(serial: String) -> Bool {
        guard let ip = getPhoneIP(serial: serial) else { return false }
        let wirelessSerial = "\(ip):5555"

        let existing = Shell.adb("devices 2>/dev/null")
        if existing.contains(wirelessSerial) { return true }

        Shell.adb("-s \(serial) tcpip 5555")

        var connected = false
        for _ in 0..<10 {
            Thread.sleep(forTimeInterval: 0.5)
            let result = Shell.adb("connect \(wirelessSerial)")
            if result.contains("connected") || result.contains("already") {
                connected = true
                break
            }
        }

        return connected
    }
}

struct Shell {
    @discardableResult
    static func run(_ command: String) -> String {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/bin/zsh")
        task.arguments = ["-c", command]
        task.environment = [
            "PATH": "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"
        ]
        let pipe = Pipe()
        task.standardOutput = pipe
        task.standardError = pipe
        do {
            try task.run()

            DispatchQueue.global().asyncAfter(deadline: .now() + 5) {
                if task.isRunning { task.terminate() }
            }

            task.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            return String(data: data, encoding: .utf8) ?? ""
        } catch {
            return ""
        }
    }

    @discardableResult
    static func adb(_ args: String) -> String {
        return run("adb \(args)")
    }
}

extension Array {
    subscript(safe index: Index) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
