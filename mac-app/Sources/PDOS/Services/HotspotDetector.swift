import Foundation
import Network

@MainActor
class HotspotDetector: ObservableObject {
    static let shared = HotspotDetector()

    enum NetworkType: String, CaseIterable {
        case wifi = "WiFi"
        case ethernet = "Ethernet"
        case cellular = "Cellular"
        case unknown = "Unknown"

        var icon: String {
            switch self {
            case .wifi: return "wifi"
            case .ethernet: return "cable.connector"
            case .cellular: return "antenna.radiowaves.left.and.right"
            case .unknown: return "questionmark.circle"
            }
        }
    }

    @Published var networkType: NetworkType = .unknown
    @Published var ssid: String?
    @Published var gatewayIP: String?
    @Published var interface: String?

    var onHotspotStateChanged: ((NetworkType) -> Void)?

    private var pathMonitor: NWPathMonitor?
    private let monitorQueue = DispatchQueue(label: "com.pdos.hotspot.monitor", qos: .utility)
    private var ssidTimer: Timer?
    private var gatewayTimer: Timer?

    private init() {}

    func startMonitoring() {
        guard pathMonitor == nil else { return }

        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            Task { @MainActor in
                self?.handlePathChange(path)
            }
        }
        monitor.start(queue: monitorQueue)
        pathMonitor = monitor

        handlePathChange(monitor.currentPath)

        ssidTimer = Timer.scheduledTimer(withTimeInterval: 4.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.detectSSID()
            }
        }

        gatewayTimer = Timer.scheduledTimer(withTimeInterval: 8.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.detectGateway()
            }
        }

        detectSSID()
        detectGateway()
    }

    func stopMonitoring() {
        pathMonitor?.cancel()
        pathMonitor = nil
        ssidTimer?.invalidate()
        ssidTimer = nil
        gatewayTimer?.invalidate()
        gatewayTimer = nil
    }

    private func handlePathChange(_ path: NWPath) {
        let newType: NetworkType
        if path.status == .satisfied {
            if path.usesInterfaceType(.wifi) {
                newType = .wifi
            } else if path.usesInterfaceType(.wiredEthernet) {
                newType = .ethernet
            } else if path.usesInterfaceType(.cellular) {
                newType = .cellular
            } else {
                newType = .unknown
            }
        } else {
            newType = .unknown
        }

        if newType != networkType {
            networkType = newType
            onHotspotStateChanged?(newType)
            if let iface = path.availableInterfaces.first(where: { $0.type == .wifi || $0.type == .wiredEthernet }) {
                interface = iface.name
            }
        }
    }

    private func detectSSID() {
        guard networkType == .wifi else {
            if ssid != nil {
                ssid = nil
            }
            return
        }

        Task(priority: .utility) {
            let output = await self.runShell("/usr/sbin/networksetup", args: ["-getairportnetwork", "en0"])
            guard let out = output else { return }

            let trimmed = out.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.contains(": ") {
                let parts = trimmed.split(separator: ":", maxSplits: 1)
                let ssidStr = parts.count > 1 ? String(parts[1]).trimmingCharacters(in: .whitespacesAndNewlines) : ""
                await MainActor.run { [weak self] in
                    self?.ssid = ssidStr.isEmpty ? nil : ssidStr
                }
            } else if trimmed.contains("not associated") || trimmed.contains("You are not") {
                await MainActor.run { [weak self] in
                    self?.ssid = nil
                }
            }
        }
    }

    private func detectGateway() {
        Task(priority: .utility) {
            let output = await self.runShell("/usr/sbin/netstat", args: ["-nr", "-f", "inet"])
            guard let out = output else { return }

            var foundGateway: String?
            for line in out.components(separatedBy: .newlines) {
                if line.hasPrefix("default") {
                    let parts = line.components(separatedBy: .whitespaces).filter { !$0.isEmpty }
                    if parts.count >= 2 {
                        foundGateway = parts[1]
                        break
                    }
                }
            }

            await MainActor.run { [weak self] in
                self?.gatewayIP = foundGateway
            }
        }
    }

    private func runShell(_ executable: String, args: [String]) async -> String? {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                let proc = Process()
                proc.executableURL = URL(fileURLWithPath: executable)
                proc.arguments = args
                let pipe = Pipe()
                proc.standardOutput = pipe
                proc.standardError = pipe
                do {
                    try proc.run()
                    proc.waitUntilExit()
                    let data = pipe.fileHandleForReading.readDataToEndOfFile()
                    let output = String(data: data, encoding: .utf8)
                    continuation.resume(returning: output)
                } catch {
                    continuation.resume(returning: nil)
                }
            }
        }
    }

    @MainActor
    deinit {
        stopMonitoring()
    }
}
