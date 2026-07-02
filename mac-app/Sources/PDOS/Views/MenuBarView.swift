import SwiftUI
import Foundation
import UniformTypeIdentifiers

struct MenuBarView: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var connectionManager: ConnectionManager
    @State private var isRelayExpanded = false
    @State private var actionFeedback: String?
    @State private var isDragOver = false
    @State private var droppedFiles: [URL] = []
    @State private var showDevicePicker = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Image(systemName: "square.stack.3d.up.fill")
                    .foregroundColor(.accentColor)
                Text("PDOS")
                    .font(.system(size: 13, weight: .semibold))
                Spacer()
                Circle()
                    .fill(connectionManager.isConnected ? Color.green : Color.gray)
                    .frame(width: 8, height: 8)
            }
            .padding(.horizontal, Spacing.lg)
            .padding(.vertical, Spacing.sm)
            .onDrop(of: [.fileURL], isTargeted: $isDragOver) { providers in
                handleMenuBarDrop(providers: providers)
                return true
            }
            .background(isDragOver ? Color.cyan.opacity(0.2) : Color.clear)

            Divider()
                .padding(.horizontal, Spacing.lg)

            if case .connected = connectionManager.connectionStatus {
                VStack(spacing: Spacing.xs) {
                    HStack {
                        Image(systemName: "cpu")
                            .font(.system(size: 12))
                            .foregroundColor(.secondary)
                            .frame(width: 20)
                        Text("CPU: \(backend.metrics?.cpu_usage ?? "--")")
                            .font(.system(size: 12))
                        Spacer()
                    }
                    .padding(.horizontal, Spacing.lg)

                    HStack {
                        Image(systemName: "memorychip")
                            .font(.system(size: 12))
                            .foregroundColor(.secondary)
                            .frame(width: 20)
                        Text("RAM: \(backend.metrics?.memory_mb ?? "--") MB")
                            .font(.system(size: 12))
                        Spacer()
                    }
                    .padding(.horizontal, Spacing.lg)
                }
                .padding(.vertical, Spacing.sm)

                Divider()
                    .padding(.horizontal, Spacing.lg)

                VStack(spacing: Spacing.xxs) {
                    MenuBarActionButton(icon: "doc.on.clipboard", title: "Copy to Clipboard") {
                        sendClipboard()
                    }
                    MenuBarActionButton(icon: "terminal", title: "Quick Terminal") {
                        openTerminal()
                    }
                    MenuBarActionButton(icon: "arrow.up.doc", title: "Send File") {
                        sendFile()
                    }
                    MenuBarActionButton(icon: "arrow.clockwise", title: "Refresh") {
                        Task { await backend.refreshAll() }
                    }
                }
                .padding(.vertical, Spacing.xs)

                if let fb = actionFeedback {
                    Text(fb)
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .padding(.horizontal, Spacing.lg)
                        .padding(.vertical, Spacing.xs)
                }

                Divider()
                    .padding(.horizontal, Spacing.lg)
            } else {
                VStack(spacing: Spacing.sm) {
                    Text(connectionManager.isConnecting ? "Connecting..." : "Disconnected")
                        .font(.subheadline)
                        .foregroundColor(.secondary)

                    if connectionManager.isConnecting {
                        ProgressView()
                            .progressViewStyle(.circular)
                            .scaleEffect(0.7)
                    }

                    Button("Start Backend") {
                        connectionManager.startRelay(backend: backend)
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)

                    if !DevicePersistence.knownRelays.isEmpty {
                        Button("Known Relays (\(DevicePersistence.knownRelays.count))") {
                            isRelayExpanded.toggle()
                        }
                        .buttonStyle(.plain)
                        .font(.caption)
                        .foregroundColor(.accentColor)

                        if isRelayExpanded {
                            ForEach(DevicePersistence.knownRelays.prefix(5)) { relay in
                                HStack {
                                    Text(relay.name)
                                        .font(.caption)
                                    Spacer()
                                    Text(relay.url)
                                        .font(.caption2)
                                        .foregroundColor(.secondary)
                                }
                                .padding(.horizontal, Spacing.lg)
                            }
                        }
                    }
                }
                .padding(.vertical, Spacing.sm)

                Divider()
                    .padding(.horizontal, Spacing.lg)
            }

            HStack(spacing: 0) {
                Button("Open PDOS") {
                    NSApp.setActivationPolicy(.regular)
                    NSApp.activate(ignoringOtherApps: true)
                    for window in NSApp.windows {
                        window.makeKeyAndOrderFront(nil)
                    }
                }
                .buttonStyle(.plain)
                .foregroundColor(.accentColor)
                .font(.system(size: 12, weight: .medium))

                Spacer()

                Button("Quit") {
                    connectionManager.stop(backend: backend)
                    NSApplication.shared.terminate(nil)
                }
                .buttonStyle(.plain)
                .font(.system(size: 12))
                .foregroundColor(.secondary)
            }
            .padding(.horizontal, Spacing.lg)
            .padding(.vertical, Spacing.md)
        }
        .frame(width: 260)
        .sheet(isPresented: $showDevicePicker) {
            DevicePickerSheet(
                files: droppedFiles,
                isPresented: $showDevicePicker,
                onSend: { deviceID in
                    sendMenuBarFiles(deviceID: deviceID)
                }
            )
        }
    }

    private func handleMenuBarDrop(providers: [NSItemProvider]) {
        droppedFiles = []
        var loadedCount = 0

        for provider in providers {
            if provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
                provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, _ in
                    guard let data = item as? Data,
                          let url = URL(dataRepresentation: data, relativeTo: nil) else { return }
                    DispatchQueue.main.async {
                        self.droppedFiles.append(url)
                        loadedCount += 1
                        if loadedCount == providers.count {
                            self.showDevicePicker = true
                        }
                    }
                }
            }
        }
    }

    private func sendMenuBarFiles(deviceID: String) {
        guard !deviceID.isEmpty, !droppedFiles.isEmpty else { return }
        actionFeedback = "Sending \(droppedFiles.count) file(s)..."
        clearFeedback()

        FileTransferService.sendFiles(droppedFiles, deviceID: deviceID) { success, _, _ in
            self.actionFeedback = "Sent \(success) file(s)"
            self.clearFeedback()
            Task { await self.backend.refreshAll() }
        }
    }

    private var dashPath: String {
        resolveDOSBinary().path
    }

    private func sendClipboard() {
        guard let text = NSPasteboard.general.string(forType: .string) else {
            actionFeedback = "No text on clipboard"
            clearFeedback()
            return
        }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: dashPath)
        task.arguments = ["clipboard", "set", text]
        do {
            try task.run()
            actionFeedback = "Clipboard sent"
        } catch {
            actionFeedback = "Clipboard failed: \(error.localizedDescription)"
        }
        clearFeedback()
    }

    private func openTerminal() {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        task.arguments = ["-e", "tell app \"Terminal\" to do script \"\(dashPath) dashboard \(backend.dashboardPort)\""]
        do {
            try task.run()
            actionFeedback = "Terminal opened"
        } catch {
            actionFeedback = "Terminal failed: \(error.localizedDescription)"
        }
        clearFeedback()
    }

    private func sendFile() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.prompt = "Send File"
        guard panel.runModal() == .OK, let url = panel.url else { return }

        let task = Process()
        task.executableURL = URL(fileURLWithPath: dashPath)
        task.arguments = ["send-file", "--all", url.path, "~/Downloads"]
        do {
            try task.run()
            actionFeedback = "Sending \(url.lastPathComponent)"
        } catch {
            actionFeedback = "Send failed: \(error.localizedDescription)"
        }
        clearFeedback()
    }

    private func clearFeedback() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            actionFeedback = nil
        }
    }
}

struct MenuBarActionButton: View {
    let icon: String
    let title: String
    let action: () -> Void

    @State private var isHovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: icon)
                    .font(.system(size: 12))
                    .foregroundColor(.secondary)
                    .frame(width: 20)
                Text(title)
                    .font(.system(size: 12))
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .background(isHovering ? Color.primary.opacity(0.1) : Color.clear)
            .clipShape(RoundedRectangle(cornerRadius: Radius.sm))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { h in isHovering = h }
    }
}
