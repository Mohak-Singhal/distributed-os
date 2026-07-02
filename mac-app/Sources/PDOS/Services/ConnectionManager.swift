import Foundation

@MainActor
class ConnectionManager: ObservableObject {
    static let shared = ConnectionManager()

    @Published var isConnecting = false
    @Published var connectionError: String?
    @Published var connectionStatus: ConnectionStatus = .disconnected

    private var retryCount = 0
    private var maxRetries = 3
    private var retryTimer: Timer?
    private var healthCheckTimer: Timer?
    private var relayProcess: Process?
    private var hotspotChangeHandler: (() -> Void)?

    enum ConnectionStatus: Equatable {
        case disconnected
        case connecting
        case connected(host: String, port: Int)
        case failed(String)

        var isConnected: Bool {
            if case .connected = self { return true }
            return false
        }
    }

    var isConnected: Bool { connectionStatus.isConnected }

    private init() {}

    func startRelay(backend: BackendService) {
        connectionStatus = .connecting
        isConnecting = true
        connectionError = nil
        retryCount = 0
        attemptRelayStart(backend: backend)
    }

    private func attemptRelayStart(backend: BackendService) {
        backend.startRelay()

        // Give the process time to initialize, then check status via its termination handler
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            guard let self = self else { return }
            if backend.relayRunning {
                self.connectionStatus = .connected(host: "127.0.0.1", port: 7890)
                self.isConnecting = false
                self.retryCount = 0
                self.startHealthCheck(backend: backend)
                if let lastRelay = DevicePersistence.lastRelay {
                    DevicePersistence.updateLastConnected(for: lastRelay.id)
                }
            } else {
                self.handleRetry(backend: backend)
            }
        }
    }

    private func handleRetry(backend: BackendService) {
        retryCount += 1
        if retryCount <= maxRetries {
            let delay = min(Double(retryCount) * 2.0, 8.0)
            connectionError = "Connection failed. Retrying in \(Int(delay))s... (Attempt \(retryCount)/\(maxRetries))"
            connectionStatus = .connecting

            retryTimer?.invalidate()
            retryTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { _ in
                Task { @MainActor in
                    ConnectionManager.shared.attemptRelayStart(backend: backend)
                }
            }
        } else {
            isConnecting = false
            connectionStatus = .failed("Could not start relay after \(maxRetries) attempts.")
            connectionError = "Relay failed to start after \(maxRetries) attempts. Check that dos-relay binary is installed."
        }
    }

    private func startHealthCheck(backend: BackendService) {
        healthCheckTimer?.invalidate()
        healthCheckTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { _ in
            Task { @MainActor in
                let mgr = ConnectionManager.shared
                if !backend.relayRunning {
                    mgr.connectionStatus = .disconnected
                    mgr.connectionError = "Relay connection lost. Reconnecting..."
                    mgr.startRelay(backend: backend)
                }
            }
        }
    }

    func stop(backend: BackendService) {
        retryTimer?.invalidate()
        retryTimer = nil
        healthCheckTimer?.invalidate()
        healthCheckTimer = nil
        relayProcess?.terminate()
        relayProcess = nil
        isConnecting = false
        retryCount = 0
        connectionStatus = .disconnected
        connectionError = nil
        backend.stopBackend()
    }

    func connectToRelay(host: String, port: Int, backend: BackendService) {
        connectionStatus = .connecting
        isConnecting = true
        connectionError = nil

        DevicePersistence.addRelay(host: host, port: port)

        let relayPath = resolveRelayBinary()
        let proc = Process()
        proc.executableURL = relayPath
        proc.arguments = ["--port", "7890", "--relay-host", host]
        relayProcess = proc

        proc.terminationHandler = { [weak self] _ in
            Task { @MainActor in
                guard let self = self else { return }
                if self.connectionStatus.isConnected {
                    self.connectionStatus = .disconnected
                    self.connectionError = "Relay process exited unexpectedly. Reconnecting..."
                    self.startRelay(backend: backend)
                }
            }
        }

        do {
            try proc.run()
            backend.relayRunning = true
            connectionStatus = .connected(host: host, port: port)
            isConnecting = false
            startHealthCheck(backend: backend)
        } catch {
            connectionStatus = .failed("Failed to launch relay: \(error.localizedDescription)")
            connectionError = error.localizedDescription
            isConnecting = false
            relayProcess = nil
        }
    }

    func setupHotspotAutoConnect(detector: HotspotDetector, backend: BackendService) {
        guard hotspotChangeHandler == nil else { return }
        hotspotChangeHandler = { [weak self, weak backend] in
            guard let self = self, let backend = backend else { return }
            if detector.networkType == .wifi && !self.isConnected && !self.isConnecting {
                self.startRelay(backend: backend)
            }
        }
        detector.onHotspotStateChanged = { [weak self] _ in
            self?.hotspotChangeHandler?()
        }
    }

    deinit {
        retryTimer?.invalidate()
        healthCheckTimer?.invalidate()
        relayProcess?.terminate()
        hotspotChangeHandler = nil
    }
}
