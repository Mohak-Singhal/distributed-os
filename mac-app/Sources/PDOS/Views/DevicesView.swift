import SwiftUI

struct DevicesView: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var connectionManager: ConnectionManager
    @State private var adbDevices: [ADBDevice] = []
    @State private var adbAvailable = false
    @State private var adbError: String?
    @State private var relayNodesList: [PDOSNode] = []
    @State private var refreshID = UUID()
    @State private var isFrontCamLoading = false
    @State private var isBackCamLoading = false
    @State private var showAddSheet = false
    @State private var isRadarScanning = false

    private let adbScanner = ADBScanner.shared
    private let nodeService = NodeService.shared

    var body: some View {
        VStack(spacing: 0) {
            if adbAvailable {
                ScrollView {
                    VStack(spacing: 16) {
                        let connected = adbDevices.filter { $0.state == "device" }
                        let activeDevice = connected.first
                        let otherConnected = connected.dropFirst()

                        if let active = activeDevice {
                            ADBDeviceRow(
                                device: active,
                                onAction: { action in
                                    handleADBAction(serial: active.serial, action: action)
                                },
                                onDisconnect: { disconnectADB(active.serial) },
                                onForget: { forgetADB(active.serial) }
                            )

                            if !otherConnected.isEmpty || !inactiveADBDevices.isEmpty || !relayNodesList.isEmpty {
                                HStack {
                                    Text("All Devices")
                                        .font(.headline)
                                        .padding(.top, 16)
                                        .padding(.bottom, 8)
                                    Spacer()
                                }

                                VStack(spacing: 12) {
                                    ForEach(Array(otherConnected)) { device in
                                        ADBDeviceRow(
                                            device: device,
                                            onAction: { action in
                                                handleADBAction(serial: device.serial, action: action)
                                            },
                                            onDisconnect: { disconnectADB(device.serial) },
                                            onForget: { forgetADB(device.serial) }
                                        )
                                    }

                                    ForEach(Array(inactiveADBDevices)) { device in
                                        ADBDeviceRow(
                                            device: device,
                                            onAction: { action in
                                                handleADBAction(serial: device.serial, action: action)
                                            },
                                            onDisconnect: { disconnectADB(device.serial) },
                                            onForget: { forgetADB(device.serial) }
                                        )
                                    }

                                    if !relayNodesList.isEmpty {
                                        networkNodesSection
                                    }
                                }
                                .padding()
                            }
                        } else {
                            airDropDiscoveryView
                        }
                    }
                }
            } else {
                airDropDiscoveryView
            }
        }
        .sheet(isPresented: $showAddSheet) {
            AddRelaySheet()
        }
        .onAppear {
            syncState()
            if case .connected = connectionManager.connectionStatus {
                nodeService.startPolling(baseURL: backend.baseURL)
            }
            startPolling()
        }
        .onChange(of: connectionManager.connectionStatus) { _, newStatus in
            if case .connected = newStatus {
                nodeService.startPolling(baseURL: backend.baseURL)
            } else {
                nodeService.stopPolling()
            }
        }
        .id(refreshID)
    }

    // MARK: - AirDrop-Style Discovery View

    private var airDropDiscoveryView: some View {
        VStack(spacing: 0) {
            Spacer()

            // Radar scanner
            RadarView(
                isScanning: isRadarScanning,
                deviceCount: allDevices.count
            )
            .frame(width: 280, height: 280)
            .padding(.bottom, 24)

            // Status text
            VStack(spacing: 8) {
                Text(isRadarScanning ? "Searching for devices..." : "Tap to start scanning")
                    .font(.title3)
                    .foregroundColor(.secondary)

                if let err = adbError {
                    Text(err)
                        .font(.caption)
                        .foregroundColor(.orange)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 40)
                }
            }
            .padding(.bottom, 32)

            // Discovered devices list
            if !allDevices.isEmpty {
                VStack(alignment: .leading, spacing: 12) {
                    HStack {
                        Text("NEARBY DEVICES")
                            .font(.system(size: 11, weight: .medium, design: .monospaced))
                            .foregroundColor(.secondary)
                            .tracking(1.2)
                        Spacer()
                        Text("\(allDevices.count)")
                            .font(.system(size: 11, weight: .bold, design: .monospaced))
                            .foregroundColor(Color.brandCyan)
                    }
                    .padding(.horizontal)

                    VStack(spacing: 8) {
                        ForEach(allDevices) { device in
                            DiscoveredDeviceRow(device: device)
                        }
                    }
                    .padding(.horizontal)
                }
                .padding(.bottom, 24)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.black.opacity(0.3))
        .onTapGesture {
            withAnimation(Anim.fadeIn) {
                isRadarScanning.toggle()
            }
        }
    }

    // MARK: - All Discovered Devices (ADB + Network)

    private var allDevices: [DiscoveredDevice] {
        var devices: [DiscoveredDevice] = []

        for adb in adbDevices {
            devices.append(DiscoveredDevice(
                id: adb.serial,
                name: adb.model.isEmpty ? adb.serial : adb.model,
                platform: "android",
                type: .usb,
                isOnline: adb.state == "device"
            ))
        }

        for node in relayNodesList {
            devices.append(DiscoveredDevice(
                id: node.nodeId,
                name: node.displayName,
                platform: node.platform,
                type: .network,
                isOnline: node.status == "online" || node.status == "device"
            ))
        }

        return devices
    }

    // MARK: - Network Nodes Section (for when ADB devices exist)

    private var networkNodesSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Network Nodes")
                    .font(.headline)
                    .padding(.top, 16)
                    .padding(.bottom, 8)
                Spacer()
            }

            VStack(spacing: 8) {
                ForEach(relayNodesList) { node in
                    NodeRow(
                        node: node,
                        onAction: { nodeId, action in
                            performAction(nodeId: nodeId, action: action)
                        },
                        onConnect: { _ in },
                        onForget: { _ in }
                    )
                }
            }
        }
    }

    private var inactiveADBDevices: [ADBDevice] {
        adbDevices.filter { $0.state != "device" }
    }

    private func startPolling() {
        Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { _ in
            Task { @MainActor in
                self.syncState()
            }
        }
    }

    private func syncState() {
        let newAvailable = adbScanner.isAvailable
        let newDevices = adbScanner.devices
        let newNodes = nodeService.nodes.filter { $0.status == "online" || $0.status == "device" }

        withAnimation(Anim.fadeIn) {
            if newAvailable != adbAvailable { adbAvailable = newAvailable }
            if newDevices != adbDevices { adbDevices = newDevices }
            if newNodes != relayNodesList { relayNodesList = newNodes }
        }

        adbError = adbScanner.scanError

        // Start radar if we have devices
        if !allDevices.isEmpty && !isRadarScanning {
            withAnimation(Anim.fadeIn) {
                isRadarScanning = true
            }
        }
    }

    private func handleADBAction(serial: String, action: String) {
        switch action {
        case "clipboard":
            if let text = NSPasteboard.general.string(forType: .string) {
                let escaped = text.replacingOccurrences(of: "'", with: "'\\''")
                _ = Shell.adb("-s \(serial) shell \"input text '\(escaped)'\"")
            }
        case "files":
            NSWorkspace.shared.open(URL(fileURLWithPath: "/System/Applications/Finder.app"))
        case "mirror":
            launchDetached("scrcpy", ["-s", serial, "--stay-awake"])
        case "frontcam":
            isFrontCamLoading = true
            launchScrcpyCamera(serial: serial, facing: "front")
            isFrontCamLoading = false
        case "backcam":
            isBackCamLoading = true
            launchScrcpyCamera(serial: serial, facing: "back")
            isBackCamLoading = false
        case "upload":
            launchDetached(resolveDOSBinary().path, ["send-file", serial, "/tmp", "~/Downloads"])
        default:
            break
        }
    }

    private func launchDetached(_ executable: String, _ args: [String]) {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        task.arguments = ["nohup", executable] + args + ["&"]
        try? task.run()
    }

    private func launchScrcpyCamera(serial: String, facing: String) {
        let defaults = UserDefaults.standard
        let smartMode = defaults.object(forKey: "smartCameraMode") as? Bool ?? true
        let rotation = defaults.string(forKey: "cameraRotation") ?? "auto"
        let cameraId = defaults.string(forKey: "cameraId") ?? "0"
        let bitRate = defaults.integer(forKey: "cameraBitrate") > 0 ? defaults.integer(forKey: "cameraBitrate") : 20000000
        let maxFps = defaults.integer(forKey: "cameraMaxFps") > 0 ? defaults.integer(forKey: "cameraMaxFps") : 30
        let orientation: String
        switch rotation {
        case "0": orientation = "0"
        case "90": orientation = "90"
        case "180": orientation = "180"
        case "270": orientation = "270"
        default: orientation = "0"
        }
        var args = ["-s", serial, "--camera-id", cameraId, "--camera-size=1920x1080", "--max-fps", "\(maxFps)", "--video-bit-rate", "\(bitRate)", "--camera-facing", facing, "--orientation", orientation, "--no-audio", "--no-window", "--turn-screen-off"]
        if smartMode {
            args.append("--camera-smart-orientation")
        }
        launchDetached("scrcpy", args)
    }

    private func disconnectADB(_ serial: String) {
        Shell.run("adb disconnect \(serial) 2>/dev/null")
        syncState()
    }

    private func forgetADB(_ serial: String) {
        Shell.run("adb disconnect \(serial) 2>/dev/null")
        DispatchQueue.main.async {
            syncState()
        }
    }

    private func performAction(nodeId: String, action: String) {
        let dashPath = resolveDOSBinary().path
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        switch action {
        case "clipboard":
            if let text = NSPasteboard.general.string(forType: .string) {
                task.arguments = ["nohup", dashPath, "send-clipboard", nodeId, text]
            }
        case "terminal":
            task.arguments = ["nohup", dashPath, "terminal", nodeId]
        case "notification":
            task.arguments = ["nohup", dashPath, "send-notification", nodeId, "Hello from Mac!", "PDOS"]
        case "ping":
            task.arguments = ["nohup", dashPath, "ping", nodeId]
        case "send-file":
            let panel = NSOpenPanel()
            panel.canChooseFiles = true
            panel.canChooseDirectories = false
            panel.allowsMultipleSelection = false
            if panel.runModal() == .OK, let url = panel.url {
                task.arguments = ["nohup", dashPath, "send-file", nodeId, url.path, "/tmp"]
            } else {
                return
            }
        case "get-file":
            task.arguments = ["nohup", dashPath, "get-file", nodeId, "/tmp/test.txt", "/tmp/"]
        default:
            return
    }
    try? task.run()
}

// MARK: - Discovered Device Row (AirDrop-style)

struct DiscoveredDeviceRow: View {
    let device: DiscoveredDevice

    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 14) {
            DeviceAvatar(
                platform: device.platform,
                name: device.name,
                isOnline: device.isOnline,
                size: 48
            )

            VStack(alignment: .leading, spacing: 3) {
                Text(device.name)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundColor(.primary)
                    .lineLimit(1)

                HStack(spacing: 6) {
                    Text(device.platformLabel)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundColor(.secondary)

                    if device.type == .usb {
                        Text("USB")
                            .font(.system(size: 10, weight: .bold, design: .monospaced))
                            .foregroundColor(Color.brandCyan)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(Capsule().fill(Color.brandCyan.opacity(0.15)))
                    }
                }
            }

            Spacer()

            Circle()
                .fill(device.isOnline ? Color.brandCyan : Color.gray.opacity(0.4))
                .frame(width: 8, height: 8)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color.surfaceCard)
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .strokeBorder(
                            isHovered ? Color.brandCyan.opacity(0.2) : Color.clear,
                            lineWidth: 1
                        )
                )
        )
        .scaleEffect(isHovered ? 1.01 : 1)
        .onHover { hovering in
            withAnimation(Anim.hover) { isHovered = hovering }
        }
    }
}

// MARK: - ADB Device Row

struct ADBDeviceRow: View {
    let device: ADBDevice
    let onAction: (String) -> Void
    let onDisconnect: () -> Void
    let onForget: () -> Void

    @State private var isFrontCamLoading = false
    @State private var isBackCamLoading = false
    @State private var isSwitchingToWireless = false
    @State private var wirelessError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                DeviceAvatar(
                    platform: "android",
                    name: device.model,
                    isOnline: device.state == "device",
                    size: 48
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(device.model.isEmpty ? device.serial : device.model)
                        .font(.headline)
                    Text(device.serial)
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                Spacer()

                if device.state == "device" {
                    Circle()
                        .fill(Color.brandCyan)
                        .frame(width: 8, height: 8)
                } else {
                    Circle()
                        .fill(Color.orange)
                        .frame(width: 8, height: 8)
                }
            }

            if device.state == "device" {
                HStack(spacing: 8) {
                    actionButton("Clipboard", icon: "clipboard") { onAction("clipboard") }
                    actionButton("Files", icon: "folder") { onAction("files") }
                    actionButton("Mirror", icon: "display") { onAction("mirror") }
                    actionButton("Front Cam", icon: "camera") {
                        isFrontCamLoading = true
                        onAction("frontcam")
                    }
                    actionButton("Back Cam", icon: "camera.fill") {
                        isBackCamLoading = true
                        onAction("backcam")
                    }
                    actionButton("Upload", icon: "arrow.up.doc") { onAction("upload") }
                }

                if let err = wirelessError {
                    Text(err)
                        .font(.caption)
                        .foregroundColor(.red)
                }
            }

            HStack(spacing: 8) {
                if device.state == "device" {
                    Button("Disconnect") { onDisconnect() }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                }
                Button("Forget", role: .destructive) { onForget() }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
        }
        .padding()
        .background(Color.surfaceCard)
        .cornerRadius(12)
    }

    private func actionButton(_ title: String, icon: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.system(size: 16))
                Text(title)
                    .font(.caption2)
            }
            .frame(minWidth: 50)
            .padding(Spacing.sm)
            .background(Color.surfaceCard)
            .cornerRadius(12)
        }
        .buttonStyle(.plain)
        .help(title)
    }
}

// MARK: - Add Relay Sheet

struct AddRelaySheet: View {
    @Environment(\.dismiss) var dismiss
    @State private var host = ""
    @State private var port = "7890"
    @State private var name = ""

    var body: some View {
        VStack(spacing: 16) {
            Text("Add Relay")
                .font(.headline)

            TextField("Host", text: $host)
                .textFieldStyle(.roundedBorder)

            TextField("Port", text: $port)
                .textFieldStyle(.roundedBorder)

            TextField("Name (optional)", text: $name)
                .textFieldStyle(.roundedBorder)

            HStack {
                Button("Cancel") { dismiss() }
                Button("Save") {
                    DevicePersistence.addRelay(
                        host: host,
                        port: Int(port) ?? 7890,
                        name: name.isEmpty ? nil : name
                    )
                    NodeService.shared.refreshKnownRelays()
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .disabled(host.isEmpty)
            }
        }
        .padding(Spacing.lg)
        .frame(width: 350, height: 280)
    }
}

}
