import SwiftUI

struct SettingsView: View {
    @AppStorage("showInMenuBar") private var showInMenuBar = false
    @AppStorage("autoConnect") private var autoConnect = true
    @State private var showingAdvanced = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                headerSection
                generalSection
                trustedDevicesSection
                advancedSection
                aboutSection
            }
            .padding(28)
        }
    }

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Settings")
                .font(.system(size: 26, weight: .bold))
                .foregroundColor(.primary)

            Text("Configure your preferences")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
    }

    private var generalSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionHeader("General")

            VStack(spacing: 0) {
                settingRow("Show in Menu Bar") {
                    Toggle("", isOn: $showInMenuBar)
                        .toggleStyle(.switch)
                        .controlSize(.small)
                }

                Divider()
                    .padding(.leading, 16)

                settingRow("Auto-connect on launch") {
                    Toggle("", isOn: $autoConnect)
                        .toggleStyle(.switch)
                        .controlSize(.small)
                }
            }
            .background(Color.primary.opacity(0.03))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }

    private var trustedDevicesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionHeader("Trusted Devices")

            VStack(spacing: 0) {
                let devices = DevicePersistence.trustedDevices
                if devices.isEmpty {
                    HStack {
                        Text("No trusted devices")
                            .font(.subheadline)
                            .foregroundColor(.secondary)
                        Spacer()
                    }
                    .padding(16)
                } else {
                    ForEach(Array(devices.enumerated()), id: \.element.id) { index, device in
                        HStack {
                            Circle()
                                .fill(Color.green)
                                .frame(width: 8, height: 8)

                            Text(device.name)
                                .font(.subheadline)

                            Spacer()

                            Text(device.lastSeen, style: .relative)
                                .font(.caption)
                                .foregroundColor(.secondary)

                            Button {
                                DevicePersistence.removeTrustedDevice(id: device.id)
                            } label: {
                                Image(systemName: "trash")
                                    .font(.caption)
                                    .foregroundColor(.red)
                            }
                            .buttonStyle(.plain)
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)

                        if index < devices.count - 1 {
                            Divider()
                                .padding(.leading, 16)
                        }
                    }
                }
            }
            .background(Color.primary.opacity(0.03))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }

    private var advancedSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionHeader("Advanced")

            VStack(spacing: 0) {
                Button {
                    withAnimation(.spring(duration: 0.3)) {
                        showingAdvanced.toggle()
                    }
                } label: {
                    HStack {
                        Image(systemName: "wrench.adjustable")
                            .font(.subheadline)
                            .foregroundColor(.brandCyan)
                            .frame(width: 24)

                        Text("Advanced Settings")
                            .font(.subheadline)
                            .foregroundColor(.primary)

                        Spacer()

                        Image(systemName: "chevron.down")
                            .font(.caption)
                            .foregroundColor(.secondary)
                            .rotationEffect(.degrees(showingAdvanced ? 180 : 0))
                    }
                    .padding(16)
                }
                .buttonStyle(.plain)

                if showingAdvanced {
                    Divider()
                        .padding(.leading, 16)

                    AdvancedSettingsContent()
                }
            }
            .background(Color.primary.opacity(0.03))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }

    private var aboutSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionHeader("About")

            VStack(spacing: 0) {
                aboutRow("Name", "PDOS Hub")
                Divider().padding(.leading, 16)
                aboutRow("Version", "1.0")
                Divider().padding(.leading, 16)
                aboutRow("Protocol", "WebSocket + Ed25519")
                Divider().padding(.leading, 16)
                aboutRow("Platform", "macOS 14+")
            }
            .background(Color.primary.opacity(0.03))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }

    // MARK: - Helpers

    private func sectionHeader(_ text: String) -> some View {
        Text(text)
            .font(.headline)
            .foregroundColor(.primary)
            .padding(.leading, 4)
    }

    private func settingRow(_ label: String, @ViewBuilder trailing: () -> some View) -> some View {
        HStack {
            Text(label)
                .font(.subheadline)
                .foregroundColor(.primary)
            Spacer()
            trailing()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private func aboutRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(.subheadline)
                .foregroundColor(.secondary)
                .frame(width: 70, alignment: .trailing)
            Text(value)
                .font(.subheadline)
                .foregroundColor(.primary)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

// MARK: - Advanced Settings Content

struct AdvancedSettingsContent: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var hotspotDetector: HotspotDetector

    @State private var knownRelays: [KnownRelay] = DevicePersistence.knownRelays
    @State private var showAddRelay = false

    var body: some View {
        VStack(spacing: 0) {
            relaySection
            networkSection
            statsSection
        }
    }

    private var relaySection: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.caption)
                    .foregroundColor(.brandCyan)
                    .frame(width: 20)
                Text("Relays")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
                Button("+") { showAddRelay = true }
                    .font(.caption)
                    .foregroundColor(.brandCyan)
                    .buttonStyle(.plain)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)

            if knownRelays.isEmpty {
                HStack {
                    Text("No relays configured")
                        .font(.caption)
                        .foregroundColor(.secondary)
                    Spacer()
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 8)
            }

            ForEach(knownRelays) { relay in
                HStack {
                    Circle().fill(Color.green).frame(width: 6, height: 6)
                    Text(relay.name)
                        .font(.caption)
                    Text(relay.url)
                        .font(.caption2)
                        .foregroundColor(.secondary)
                    Spacer()
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 4)
            }

            if !knownRelays.isEmpty {
                Divider()
                    .padding(.leading, 36)
            }
        }
        .sheet(isPresented: $showAddRelay) {
            addRelaySheet
        }
    }

    private var networkSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            Divider()
                .padding(.leading, 36)

            HStack {
                Image(systemName: "network")
                    .font(.caption)
                    .foregroundColor(.brandCyan)
                    .frame(width: 20)
                Text("Network")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)

            infoRow("WiFi", hotspotDetector.ssid ?? "Not connected")
            infoRow("Gateway", hotspotDetector.gatewayIP ?? "--")
            infoRow("Type", hotspotDetector.networkType.rawValue)
        }
    }

    private var statsSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            Divider()
                .padding(.leading, 36)

            HStack {
                Image(systemName: "chart.bar")
                    .font(.caption)
                    .foregroundColor(.brandCyan)
                    .frame(width: 20)
                Text("Transfers")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)

            let history = backend.history
            if history.isEmpty {
                infoRow("Status", "No transfers yet")
            } else {
                let completed = history.filter { $0.status == "completed" }.count
                let failed = history.filter { $0.status == "failed" }.count
                infoRow("Total", "\(history.count)")
                infoRow("Completed", "\(completed)")
                infoRow("Failed", "\(failed)")
            }
        }
    }

    private func infoRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundColor(.secondary)
                .frame(width: 60, alignment: .trailing)
            Text(value)
                .font(.caption)
                .foregroundColor(.primary)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 3)
    }

    private var addRelaySheet: some View {
        VStack(spacing: 16) {
            Text("Add Relay").font(.title2).bold()
            TextField("Name", text: .constant(""))
                .textFieldStyle(.roundedBorder)
            TextField("Host", text: .constant(""))
                .textFieldStyle(.roundedBorder)
            TextField("Port", text: .constant("7890"))
                .textFieldStyle(.roundedBorder)

            HStack {
                Button("Cancel") { showAddRelay = false }
                Button("Save") { showAddRelay = false }
                    .buttonStyle(.borderedProminent)
            }
        }
        .padding(24)
        .frame(width: 360, height: 260)
    }
}
