import Foundation
import UserNotifications

@MainActor
class BackendService: ObservableObject {
    static let shared = BackendService()

    @Published var metrics: SystemMetrics?
    @Published var healthScore: HealthScore?
    @Published var transfers: [TransferSession] = []
    @Published var bottleneck: Bottleneck?
    @Published var bufferAnalysis: BufferAnalysis?
    @Published var history: [TransferSession] = []
    @Published var transferProgress: TransferProgress?
    @Published var senderTransferProgress: TransferProgress?
    @Published var remoteProgress: TransferProgress?
    @Published var errorMessage: String?
    @Published var relayRunning = false
    @Published var dashboardPort: Int = 8080
    @Published var relayPort: Int = 7890

    // MARK: - Capability Handshake
    @Published var peerCapabilities: CapabilityExchange?
    @Published var ourCapabilities: CapabilityExchange?
    @Published var telemetry: DynamicTelemetry?
    @Published var handshakeComplete = false

    // MARK: - SSE Stream
    private var sseTask: Task<Void, Never>?
    @Published var sseConnected = false

    private var pollTimer: Timer?
    private var relayProcess: Process?
    private var dashboardProcess: Process?
    var baseURL: String { "http://127.0.0.1:\(dashboardPort)" }

    private let defaultRelayPath = resolveRelayBinary().path
    private let defaultDashboardPath = resolveDOSBinary().path

    private init() {}

    // MARK: - Capability Handshake

    func performHandshake(host: String, port: Int) async -> Bool {
        guard let url = URL(string: "http://\(host):\(port)/api/handshake") else { return false }

        // Build our capabilities from local system info
        let hw = HardwareCapabilities(
            cpu_architecture: "arm64",
            cpu_cores: ProcessInfo.processInfo.processorCount,
            cpu_performance_cores: ProcessInfo.processInfo.processorCount,
            cpu_efficiency_cores: 0,
            ram_mb: Int(ProcessInfo.processInfo.physicalMemory / 1_048_576),
            storage_type: "ssd",
            storage_read_mbps: 3000,
            storage_write_mbps: 2500,
            storage_free_gb: 100
        )
        let net = NetworkCapabilities(
            interface_type: "WiFi6",
            link_speed_mbps: 1200,
            rtt_ms: 0,
            measured_bandwidth_mbps: 0,
            mtu: 1500
        )
        let state = DynamicTelemetry(
            battery_pct: 100,
            charging: true,
            thermal_state: "nominal",
            cpu_load_pct: 0,
            memory_pressure: "low",
            disk_utilization_pct: 0
        )
        let features = SupportedFeatures(
            zero_copy: true,
            parallel_upload: true,
            parallel_download: true,
            resume: true,
            streaming_directory: true,
            compression: ["none"],
            integrity: ["sha256"],
            http2: false,
            http3: false
        )
        let caps = CapabilityExchange(
            protocol_version: "1.0",
            node_id: "mac-\(ProcessInfo.processInfo.hostName)",
            hardware: hw,
            network: net,
            state: state,
            features: features
        )

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONEncoder().encode(caps)

        do {
            let (data, resp) = try await URLSession.shared.data(for: request)
            guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else { return false }
            let peer = try JSONDecoder().decode(CapabilityExchange.self, from: data)
            peerCapabilities = peer
            ourCapabilities = caps
            handshakeComplete = true
            return true
        } catch {
            return false
        }
    }

    // MARK: - SSE Control Stream

    func startSSEStream(transferId: String) {
        sseTask?.cancel()
        sseTask = Task { [weak self] in
            guard let self = self else { return }
            guard let url = URL(string: "\(self.baseURL)/api/control-stream?id=\(transferId)") else { return }
            do {
                let (bytes, response) = try await URLSession.shared.bytes(from: url)
                guard let http = response as? HTTPURLResponse, http.statusCode == 200 else { return }
                self.sseConnected = true

                for try await line in bytes.lines {
                    if line.hasPrefix("data: ") {
                        let json = String(line.dropFirst(6))
                        if let data = json.data(using: .utf8),
                           let event = try? JSONDecoder().decode(SSEEvent.self, from: data) {
                            await MainActor.run {
                                self.transferProgress = event.progress
                                if let tel = event.telemetry {
                                    self.telemetry = DynamicTelemetry(
                                        battery_pct: tel.battery_pct,
                                        charging: true,
                                        thermal_state: tel.thermal_state,
                                        cpu_load_pct: tel.cpu_load_pct,
                                        memory_pressure: tel.memory_pressure,
                                        disk_utilization_pct: 0
                                    )
                                }
                            }
                        }
                    }
                }
            } catch {
                await MainActor.run { self.sseConnected = false }
            }
        }
    }

    func stopSSEStream() {
        sseTask?.cancel()
        sseTask = nil
        sseConnected = false
    }

    // MARK: - Telemetry

    func fetchTelemetry() async -> DynamicTelemetry? {
        await fetchJSON("/api/telemetry", as: DynamicTelemetry.self)
    }

    func startPolling() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            Task { [weak self] in await self?.refreshAll() }
        }
    }

    func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }

    private var lastCompletedTransfers: Int = 0

    func requestNotificationPermission() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func refreshAll() async {
        let oldTransferCount = lastCompletedTransfers
        await fetchSystemMetrics()
        await fetchHealthScore()
        await fetchTransfers()
        await fetchBottleneck()
        await fetchBufferAnalysis()
        await fetchHistory()
        await fetchTransferProgress()
        await fetchSenderTransferProgress()
        await fetchRemoteProgress()

        if oldTransferCount >= 0 && history.count > oldTransferCount {
            let newCount = history.count - oldTransferCount
            for session in history.suffix(newCount) {
                if session.status == "completed" || session.status == "complete" {
                    sendTransferNotification(filename: session.filename ?? "File", success: true)
                } else if session.status == "failed" || session.status == "error" {
                    sendTransferNotification(filename: session.filename ?? "File", success: false)
                }
            }
        }
        lastCompletedTransfers = history.count
    }

    private func sendTransferNotification(filename: String, success: Bool) {
        let content = UNMutableNotificationContent()
        content.title = success ? "PDOS: Transfer Complete" : "PDOS: Transfer Failed"
        content.body = success ? "\(filename) transferred successfully" : "\(filename) transfer failed"
        content.sound = .default
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    func fetchTransferProgress() async {
        if let p = await fetchJSON("/api/transfer-progress", as: TransferProgress.self) {
            transferProgress = p
        }
    }

    func fetchSenderTransferProgress() async {
        if let p = await fetchJSON("/api/sender-transfer-progress", as: TransferProgress.self) {
            senderTransferProgress = p
        }
    }

    func fetchRemoteProgress() async {
        if let p = await fetchJSON("/api/sender-progress", as: TransferProgress.self) {
            remoteProgress = p
        }
    }

    private func fetchJSON<T: Decodable>(_ path: String, as type: T.Type) async -> T? {
        guard let url = URL(string: "\(baseURL)\(path)") else { return nil }
        do {
            let (data, resp) = try await URLSession.shared.data(from: url)
            guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else { return nil }
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            return nil
        }
    }

    func fetchSystemMetrics() async {
        if let m = await fetchJSON("/api/system-metrics", as: SystemMetrics.self), m != metrics {
            metrics = m
        }
    }

    func fetchHealthScore() async {
        if let h = await fetchJSON("/api/health-score", as: HealthScore.self), h != healthScore {
            healthScore = h
        }
    }

    func fetchTransfers() async {
        if let t = await fetchJSON("/api/transfer-status", as: [TransferSession].self) {
            transfers = t
        }
    }

    func fetchBottleneck() async {
        if let b = await fetchJSON("/api/bottleneck", as: Bottleneck.self), b != bottleneck {
            bottleneck = b
        }
    }

    func fetchBufferAnalysis() async {
        if let b = await fetchJSON("/api/buffer-analysis", as: BufferAnalysis.self) {
            bufferAnalysis = b
        }
    }

    func fetchHistory() async {
        if let h = await fetchJSON("/api/transfer-history", as: [TransferSession].self) {
            history = h
        }
    }

    func downloadFileHTTP(host: String, port: Int, remotePath: String, localDir: String) async -> Bool {
        guard let url = URL(string: "\(baseURL)/api/download-from-device") else { return false }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = [
            "host": host,
            "port": port,
            "remote_path": remotePath,
            "local_dir": localDir
        ]
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        guard let (_, resp) = try? await URLSession.shared.data(for: request),
              let http = resp as? HTTPURLResponse, http.statusCode == 200 else { return false }
        return true
    }

    func sendFileHTTP(host: String, port: Int, localPath: String, filename: String?) async -> Bool {
        guard let url = URL(string: "\(baseURL)/api/send-to-device") else { return false }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = [
            "host": host,
            "port": port,
            "local_path": localPath,
            "filename": filename ?? URL(fileURLWithPath: localPath).lastPathComponent
        ]
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        guard let (_, resp) = try? await URLSession.shared.data(for: request),
              let http = resp as? HTTPURLResponse, http.statusCode == 200 else { return false }
        return true
    }

    func fetchProtocolStats() async -> ProtocolStats? {
        await fetchJSON("/api/protocol-stats", as: ProtocolStats.self)
    }

    func fetchStorageForecast(path: String, size: Int) async -> StorageForecast? {
        let enc = path.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? path
        return await fetchJSON("/api/storage-forecast?path=\(enc)&size=\(size)", as: StorageForecast.self)
    }

    func fetchNetworkPaths() async -> [[String: Any]] {
        guard let url = URL(string: "\(baseURL)/api/network-path") else { return [] }
        guard let d = try? Data(contentsOf: url),
              let json = try? JSONSerialization.jsonObject(with: d) as? [[String: Any]] else { return [] }
        return json
    }

    func startRelay() {
        guard !relayRunning else { return }
        // Start relay binary (bundled or from PATH)
        let relayPath = resolveRelayBinary().path
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: relayPath)
        proc.arguments = ["--port", "\(relayPort)"]
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = FileHandle.nullDevice
        do {
            try proc.run()
            relayProcess = proc
            relayRunning = true
            errorMessage = nil
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
                self?.startDashboard()
            }
        } catch {
            errorMessage = "Failed to start relay: \(error.localizedDescription)"
        }
    }

    func startDashboard() {
        // Start dashboard binary (bundled or from PATH)
        let dashPath = resolveDOSBinary().path
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: dashPath)
        proc.arguments = ["dashboard", "\(dashboardPort)"]
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = FileHandle.nullDevice
        do {
            try proc.run()
            dashboardProcess = proc
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
                self?.startPolling()
            }
        } catch {
            errorMessage = "Failed to start dashboard: \(error.localizedDescription)"
        }
    }

    func stopBackend() {
        stopPolling()
        dashboardProcess?.terminate()
        dashboardProcess = nil
        relayProcess?.terminate()
        relayProcess = nil
        relayRunning = false
    }

    func restartBackend() {
        stopBackend()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            self?.startRelay()
        }
    }
}
