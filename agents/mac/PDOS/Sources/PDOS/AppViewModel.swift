import Foundation
import Combine
import AppKit

// MARK: - API Models

struct NodeDevice: Codable, Identifiable {
    var id: String { node_id }
    let node_id: String
    let name: String
    let platform: String
    let status: String
    let version: String?
    let score: Double
}

struct SystemMetrics: Codable {
    let cpu_usage: String
    let global_cpu: String?
    let memory_mb: String
    let total_memory_mb: String?
    let used_memory_mb: String?
    let network_tx_mbps: String
    let network_rx_mbps: String
    let android_cpu: String?
    let android_ram: String?
    let processes: ProcessMetrics?
    let thermal: ThermalMetrics?
    let disks: [DiskInfo]?
    let interfaces: [NetworkInterface]?
    let protocol_stats: ProtocolStats?
    let logs: [LogEntry]
}

struct ProcessMetrics: Codable {
    let rust_daemon: ThreadMetric?
    let hash_thread: ThreadMetric?
}

struct ThreadMetric: Codable {
    let cpu: Double?
    let ram_mb: Double?
}

struct ThermalMetrics: Codable {
    let cpu_temp_c: Double?
    let thermal_state: String?
    let fan_rpm: Double?
    let battery_pct: Double?
    let battery_temp_c: Double?
}

struct DiskInfo: Codable {
    let name: String
    let mount: String
    let total_gb: Double
    let available_gb: Double
    let file_system: String
}

struct NetworkInterface: Codable {
    let name: String
    let rx: Int
    let tx: Int
    let mac: String
}

struct ProtocolStats: Codable {
    let discovery_packets: Int?
    let auth_requests: Int?
    let transfer_requests: Int?
    let resume_requests: Int?
    let cancelled_transfers: Int?
    let completed_transfers: Int?
    let failed_transfers: Int?
    let tls_handshakes: Int?
    let range_requests: Int?
}

struct LogEntry: Codable, Identifiable {
    var id: String { "\(time)_\(msg)" }
    let time: String
    let level: String
    let msg: String
}

struct TransferHistoryItem: Codable, Identifiable {
    var id: String { filename + (start_time ?? "") }
    let filename: String
    let size: Int?
    let duration_secs: Double?
    let average_speed_mbps: Double?
    let peak_speed_mbps: Double?
    let health_score: Double?
    let completed: Bool?
    let verified: Bool?
    let start_time: String?
    let end_time: String?
    let bottleneck: String?
    let compression_ratio: Double?
    let compressed_size: Int?
    let bandwidth_saved: Int?
    let reconnects: Int?
}

struct TransferReport: Codable {
    let transfer_summary: TransferSummary?
    let file: FileInfo?
    let transfer: TransferSpeed?
    let network: NetworkInfo?
    let resources: ResourceInfo?
    let result: TransferResult?
    let health: HealthInfo?
    let waterfall: [PhaseItem]?
    let speed_samples: [SpeedSample]?
    let network_changes: [NetworkChangeItem]?
}

struct TransferSummary: Codable {
    let transfer_id: String?
    let start_time: String?
    let end_time: String?
    let duration_secs: Double?
}

struct FileInfo: Codable {
    let name: String?
    let type: String?
    let fileExtension: String?
    let sha256: String?
    let original_size: Int?
    let compressed_size: Int?
    let compression_ratio: Double?

    enum CodingKeys: String, CodingKey {
        case name, type, sha256, original_size, compressed_size, compression_ratio
        case fileExtension = "extension"
    }
}

struct TransferSpeed: Codable {
    let average_speed_mbps: Double?
    let peak_speed_mbps: Double?
    let min_speed_mbps: Double?
    let median_speed_mbps: Double?
    let p95_speed_mbps: Double?
}

struct NetworkInfo: Codable {
    let average_rtt_ms: Double?
    let peak_rtt_ms: Double?
    let packet_loss_pct: Double?
    let retransmissions: Int?
    let reconnects: Int?
}

struct ResourceInfo: Codable {
    let average_cpu_pct: Double?
    let peak_cpu_pct: Double?
    let average_ram_mb: Double?
    let peak_ram_mb: Double?
    let disk_read_mbps: Double?
    let disk_write_mbps: Double?
}

struct TransferResult: Codable {
    let completed: Bool?
    let verified: Bool?
    let resumed: Bool?
    let interrupted: Bool?
    let error: String?
}

struct HealthInfo: Codable {
    let health_score: Double?
    let bottleneck: String?
    let recommendation: String?
}

struct PhaseItem: Codable, Identifiable {
    var id: String { name }
    let name: String
    let start_ms: Double?
    let end_ms: Double?
    let duration_ms: Double?
}

struct SpeedSample: Codable, Identifiable {
    var id: String { time_offset_sec.map { "\($0)" } ?? UUID().uuidString }
    let time_offset_sec: Double?
    let speed_mbps: Double?
}

struct NetworkChangeItem: Codable, Identifiable {
    var id: String { time + interface }
    let time: String
    let interface: String
    let ip: String?
    let rssi: Double?
    let link_speed: Double?
    let event: String?
}

struct HealthScore: Codable {
    let overall: Double?
    let cpu: Double?
    let network: Double?
    let disk: Double?
    let integrity: Double?
    let recovery: Double?
}

struct NetworkPathItem: Codable, Identifiable {
    var id: String { (time ?? "") + (interface ?? "") + UUID().uuidString }
    let time: String?
    let interface: String?
    let ip: String?
    let rssi: Double?
    let link_speed: Double?
    let event: String?
}

struct StorageForecast: Codable {
    let total_gb: Double?
    let free_gb: Double?
    let file_size_gb: Double?
    let remaining_gb: Double?
    let enough_space: Bool?
}

struct CompressionItem: Codable, Identifiable {
    var id: String { filename }
    let filename: String
    let original_size: Int?
    let compressed_size: Int?
    let compression_ratio: Double?
    let compression_time_ms: Int?
    let bandwidth_saved: Int?
    let time_saved_sec: Double?
}

struct BottleneckResult: Codable {
    let bottleneck: String?
    let recommendation: String?
    let health_score: Double?
}

struct BufferMetrics: Codable {
    let read_buffer_kb: Int?
    let write_buffer_kb: Int?
    let average_queue_depth: Double?
    let max_queue_depth: Int?
    let backpressure_events: Int?
}

// MARK: - ViewModel

class AppViewModel: ObservableObject {
    @Published var isScanning = false
    @Published var discoveredNodes: [NodeDevice] = []
    @Published var selectedNode: NodeDevice? = nil
    @Published var isConnected = false

    @Published var isTransferring = false
    @Published var systemMetrics: SystemMetrics? = nil
    @Published var healthScore: HealthScore? = nil
    @Published var transferHistory: [TransferHistoryItem] = []
    @Published var selectedReport: TransferReport? = nil
    @Published var networkPath: [NetworkPathItem] = []
    @Published var storageInfo: StorageForecast? = nil
    @Published var compressionAnalytics: [CompressionItem] = []
    @Published var bottleneck: BottleneckResult? = nil
    @Published var bufferMetrics: BufferMetrics? = nil
    @Published var protocolStats: ProtocolStats? = nil

    private var scanTimer: AnyCancellable?
    private var telemetryTimer: AnyCancellable?
    private let baseURL = "http://127.0.0.1:8080/api"

    func toggleScan() {
        isScanning.toggle()
        if isScanning {
            scanDevices()
            scanTimer = Timer.publish(every: 3.0, on: .main, in: .common).autoconnect().sink { [weak self] _ in
                self?.scanDevices()
            }
        } else {
            scanTimer?.cancel()
            scanTimer = nil
        }
    }

    func scanDevices() {
        guard let url = URL(string: "\(baseURL)/devices") else { return }
        URLSession.shared.dataTask(with: url) { data, _, error in
            guard let data = data, error == nil else { return }
            DispatchQueue.main.async {
                if let devices = try? JSONDecoder().decode([NodeDevice].self, from: data) {
                    self.discoveredNodes = devices
                    if let android = devices.first(where: { $0.platform.lowercased() == "android" }) {
                        self.selectedNode = android
                        self.isConnected = true
                    }
                }
            }
        }.resume()
    }

    private var sleepActivityToken: NSObjectProtocol? = nil
    private var activeTransfersCount = 0 {
        didSet {
            DispatchQueue.main.async {
                self.isTransferring = self.activeTransfersCount > 0
                if self.isTransferring {
                    self.acquirePowerAssertion()
                } else {
                    self.releasePowerAssertion()
                }
            }
        }
    }

    private func acquirePowerAssertion() {
        guard sleepActivityToken == nil else { return }
        sleepActivityToken = ProcessInfo.processInfo.beginActivity(
            options: [.idleSystemSleepDisabled, .suddenTerminationDisabled],
            reason: "PDOS active file transfer"
        )
    }

    private func releasePowerAssertion() {
        if let token = sleepActivityToken {
            ProcessInfo.processInfo.endActivity(token)
            sleepActivityToken = nil
        }
    }

    func sendFiles(urls: [URL]) {
        guard let node = selectedNode else { return }
        
        if node.node_id.contains(".") || node.node_id.contains(":") {
            let port = (node.platform.lowercased() == "android" || node.platform.lowercased() == "unknown") ? 7891 : 8080
            sendFilesDirectly(urls: urls, toIp: node.node_id, port: port)
            return
        }
        
        let urlList = urls
        DispatchQueue.main.async {
            self.activeTransfersCount += urlList.count
        }
        for url in urlList {
            guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
                  let fileSize = attrs[.size] as? NSNumber else {
                DispatchQueue.main.async {
                    self.activeTransfersCount -= 1
                }
                continue
            }
            let filename = url.lastPathComponent.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? url.lastPathComponent
            guard let uploadURL = URL(string: "\(baseURL)/stream-upload?node_id=\(node.node_id)&filename=\(filename)") else {
                DispatchQueue.main.async {
                    self.activeTransfersCount -= 1
                }
                continue
            }
            var req = URLRequest(url: uploadURL)
            req.httpMethod = "POST"
            req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            req.setValue("\(fileSize.intValue)", forHTTPHeaderField: "Content-Length")
            req.setValue(filename, forHTTPHeaderField: "X-Filename")
            URLSession.shared.uploadTask(with: req, fromFile: url) { _, _, _ in
                DispatchQueue.main.async {
                    self.activeTransfersCount -= 1
                    self.pollTransferHistory()
                }
            }.resume()
        }
    }

    func sendFilesDirectly(urls: [URL], toIp ip: String, port: Int = 7894) {
        let urlList = urls
        DispatchQueue.main.async {
            self.activeTransfersCount += urlList.count
        }
        for url in urlList {
            guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
                  let fileSize = attrs[.size] as? NSNumber else {
                DispatchQueue.main.async {
                    self.activeTransfersCount -= 1
                }
                continue
            }
            let filename = url.lastPathComponent.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? url.lastPathComponent
            guard let uploadURL = URL(string: "http://\(ip):\(port)/api/receive-file") else {
                DispatchQueue.main.async {
                    self.activeTransfersCount -= 1
                }
                continue
            }
            var req = URLRequest(url: uploadURL)
            req.httpMethod = "POST"
            req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            req.setValue("\(fileSize.intValue)", forHTTPHeaderField: "Content-Length")
            req.setValue(filename, forHTTPHeaderField: "X-Filename")
            URLSession.shared.uploadTask(with: req, fromFile: url) { _, _, _ in
                DispatchQueue.main.async {
                    self.activeTransfersCount -= 1
                    self.pollTransferHistory()
                }
            }.resume()
        }
    }

    func connectToIP(_ ipString: String, completion: @escaping (Bool, String?) -> Void) {
        let ip = ipString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !ip.isEmpty else {
            completion(false, "IP address cannot be empty")
            return
        }
        
        let handshakeBody: [String: Any] = [
            "protocol_version": "1.0",
            "node_id": "PDOS-Mac-Client",
            "hardware": [
                "cpu_architecture": "arm64",
                "cpu_cores": 8,
                "ram_mb": 16384,
                "storage_type": "SSD",
                "storage_read_mbps": 3000.0,
                "storage_write_mbps": 2500.0,
                "storage_free_gb": 100.0
            ],
            "network": [
                "interface_type": "wifi",
                "link_speed_mbps": 1000.0,
                "rtt_ms": 2.0,
                "measured_bandwidth_mbps": 500.0,
                "mtu": 1500
            ],
            "state": [
                "battery_pct": 100.0,
                "charging": true,
                "thermal_state": "nominal",
                "cpu_load_pct": 10.0,
                "memory_pressure": "nominal",
                "disk_utilization_pct": 50.0
            ],
            "features": [
                "zero_copy": true,
                "parallel_upload": true,
                "parallel_download": true,
                "resume": true,
                "streaming_directory": true,
                "compression": [] as [String],
                "integrity": [] as [String],
                "http2": true,
                "http3": false
            ]
        ]
        
        guard let bodyData = try? JSONSerialization.data(withJSONObject: handshakeBody, options: []) else {
            completion(false, "Serialization error")
            return
        }
        
        let ports = [8080, 7891, 8443, 7894]
        
        func tryPort(index: Int) {
            guard index < ports.count else {
                completion(false, "Handshake failed on all ports")
                return
            }
            let port = ports[index]
            let urlString = "http://\(ip):\(port)/api/handshake"
            guard let url = URL(string: urlString) else {
                tryPort(index: index + 1)
                return
            }
            
            var request = URLRequest(url: url)
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = bodyData
            request.timeoutInterval = 3.0
            
            URLSession.shared.dataTask(with: request) { data, response, error in
                if let error = error {
                    print("Handshake failed on \(ip):\(port) with error: \(error)")
                    tryPort(index: index + 1)
                    return
                }
                
                guard let httpResponse = response as? HTTPURLResponse,
                      httpResponse.statusCode == 200,
                      let data = data else {
                    tryPort(index: index + 1)
                    return
                }
                
                if let json = try? JSONSerialization.jsonObject(with: data, options: []) as? [String: Any],
                   let remoteNodeId = json["node_id"] as? String {
                    
                    let platformObj = json["network"] as? [String: Any]
                    let platform = (platformObj?["interface_type"] as? String) ?? "Unknown"
                    
                    DispatchQueue.main.async {
                        let newDevice = NodeDevice(
                            node_id: ip,
                            name: remoteNodeId,
                            platform: platform,
                            status: "Online",
                            version: "1.0",
                            score: 100.0
                        )
                        self.discoveredNodes.removeAll(where: { $0.node_id == ip })
                        self.discoveredNodes.append(newDevice)
                        self.selectedNode = newDevice
                        self.isConnected = true
                        completion(true, remoteNodeId)
                    }
                } else {
                    tryPort(index: index + 1)
                }
            }.resume()
        }
        
        tryPort(index: 0)
    }

    func startTelemetryPolling() {
        telemetryTimer = Timer.publish(every: 3.0, on: .main, in: .common).autoconnect().sink { [weak self] _ in
            self?.pollAll()
        }
    }

    func stopTelemetryPolling() {
        telemetryTimer?.cancel()
        telemetryTimer = nil
    }

    private func pollAll() {
        pollMetrics()
        pollHealthScore()
        pollTransferHistory()
        pollProtocolStats()
        pollNetworkPath()
        pollStorageForecast()
        pollCompressionAnalytics()
        pollBottleneck()
        pollBufferMetrics()
    }

    private func pollMetrics() {
        getJSON("\(baseURL)/system-metrics") { (metrics: SystemMetrics?) in
            DispatchQueue.main.async { self.systemMetrics = metrics }
        }
    }

    func pollHealthScore() {
        getJSON("\(baseURL)/health-score") { (score: HealthScore?) in
            DispatchQueue.main.async { self.healthScore = score }
        }
    }

    func pollTransferHistory() {
        getJSON("\(baseURL)/transfer-history") { (items: [TransferHistoryItem]?) in
            DispatchQueue.main.async { self.transferHistory = items ?? [] }
        }
    }

    func loadTransferReport(id: String) {
        let url = "\(baseURL)/transfer-report?id=\(id.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? id)"
        getJSON(url) { (report: TransferReport?) in
            DispatchQueue.main.async { self.selectedReport = report }
        }
    }

    func pollProtocolStats() {
        getJSON("\(baseURL)/protocol-stats") { (stats: ProtocolStats?) in
            DispatchQueue.main.async { self.protocolStats = stats }
        }
    }

    func pollNetworkPath() {
        getJSON("\(baseURL)/network-path") { (items: [NetworkPathItem]?) in
            DispatchQueue.main.async { self.networkPath = items ?? [] }
        }
    }

    func pollStorageForecast() {
        getJSON("\(baseURL)/storage-forecast?path=/tmp&size=0") { (info: StorageForecast?) in
            DispatchQueue.main.async { self.storageInfo = info }
        }
    }

    func pollCompressionAnalytics() {
        getJSON("\(baseURL)/compression-analytics") { (items: [CompressionItem]?) in
            DispatchQueue.main.async { self.compressionAnalytics = items ?? [] }
        }
    }

    func pollBottleneck() {
        getJSON("\(baseURL)/bottleneck") { (result: BottleneckResult?) in
            DispatchQueue.main.async { self.bottleneck = result }
        }
    }

    func pollBufferMetrics() {
        getJSON("\(baseURL)/buffer-analysis") { (metrics: BufferMetrics?) in
            DispatchQueue.main.async { self.bufferMetrics = metrics }
        }
    }

    func exportSession(id: String) {
        let url = "\(baseURL)/export-session?id=\(id.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? id)"
        guard let reqURL = URL(string: url) else { return }
        URLSession.shared.dataTask(with: reqURL) { data, _, _ in
            guard let data = data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
            let desktop = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask).first!
            let fileURL = desktop.appendingPathComponent("pdos_session_\(id).json")
            try? (try? JSONSerialization.data(withJSONObject: json, options: .prettyPrinted))?.write(to: fileURL)
            DispatchQueue.main.async {
                NSWorkspace.shared.activateFileViewerSelecting([fileURL as URL])
            }
        }.resume()
    }

    private func getJSON<T: Codable>(_ url: String, completion: @escaping (T?) -> Void) {
        guard let url = URL(string: url) else { completion(nil); return }
        URLSession.shared.dataTask(with: url) { data, _, _ in
            guard let data = data else { completion(nil); return }
            completion(try? JSONDecoder().decode(T.self, from: data))
        }.resume()
    }
}
