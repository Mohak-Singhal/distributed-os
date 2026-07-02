import SwiftUI
import SharingService

struct ShareExtensionView: View {
    let controller: ShareViewController
    @State private var knownDevices: [Device] = []
    @State private var selectedDevice: Device?
    @State private var isLoading = true
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 20) {
            headerView

            if isLoading {
                ProgressView("Scanning for devices...")
                    .frame(height: 100)
            } else if let error = errorMessage {
                errorView(error)
            } else if knownDevices.isEmpty {
                emptyView
            } else {
                deviceListView
            }
        }
        .padding(24)
        .frame(width: 320, height: 380)
        .onAppear { loadDevices() }
    }

    private var headerView: some View {
        HStack {
            Image(systemName: "square.stack.3d.up.fill")
                .font(.title)
                .foregroundColor(.accentColor)
            Text("Send via PDOS")
                .font(.title2)
                .bold()
        }
    }

    private func errorView(_ error: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle")
                .font(.largeTitle)
                .foregroundColor(.orange)
            Text(error)
                .font(.caption)
                .foregroundColor(.secondary)
            Button("Retry") { loadDevices() }
                .buttonStyle(.bordered)
        }
        .frame(height: 100)
    }

    private var emptyView: some View {
        VStack(spacing: 8) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .font(.largeTitle)
                .foregroundColor(.secondary)
            Text("No devices found")
                .font(.subheadline)
            Text("Make sure PDOS is running on your devices")
                .font(.caption)
                .foregroundColor(.secondary)
            Button("Retry") { loadDevices() }
                .buttonStyle(.bordered)
        }
        .frame(height: 120)
    }

    private var deviceListView: some View {
        VStack(spacing: 12) {
            Text("Select target device:")
                .font(.subheadline)
                .foregroundColor(.secondary)

            ScrollView {
                VStack(spacing: 8) {
                    ForEach(knownDevices) { device in
                        Button(action: { selectedDevice = device }) {
                            DeviceSelectionRow(device: device, isSelected: selectedDevice?.id == device.id)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            if let device = selectedDevice {
                Button(action: { sendToSelectedDevice(device: device) }) {
                    HStack {
                        Image(systemName: "paperplane.fill")
                        Text("Send to \(device.name)")
                    }
                    .frame(maxWidth: .infinity)
                    .padding()
                }
                .buttonStyle(.borderedProminent)
            }
        }
    }

    private func loadDevices() {
        isLoading = true
        errorMessage = nil

        FileTransferService.listDevices { devices in
            isLoading = false
            if devices.isEmpty {
                errorMessage = "No devices found"
            } else {
                knownDevices = devices
            }
        }
    }

    private func sendToSelectedDevice(device: Device) {
        guard let inputItems = controller.extensionContext?.inputItems as? [NSExtensionItem] else {
            controller.cancelWithError(NSError(domain: "PDOS", code: 1, userInfo: [NSLocalizedDescriptionKey: "No input items"]))
            return
        }

        let group = DispatchGroup()
        var fileURLs: [URL] = []

        for item in inputItems {
            if let attachments = item.attachments {
                for attachment in attachments {
                    if attachment.hasItemConformingToTypeIdentifier("public.file-url") {
                        group.enter()
                        attachment.loadItem(forTypeIdentifier: "public.file-url", options: nil) { item, _ in
                            defer { group.leave() }
                            if let url = item as? URL {
                                fileURLs.append(url)
                            } else if let data = item as? Data,
                                      let url = URL(dataRepresentation: data, relativeTo: nil) {
                                fileURLs.append(url)
                            }
                        }
                    }
                }
            }
        }

        group.notify(queue: .main) {
            for url in fileURLs {
                self.controller.sendFile(url: url, deviceID: device.id)
            }
        }
    }
}

struct DeviceSelectionRow: View {
    let device: Device
    let isSelected: Bool

    var body: some View {
        HStack {
            Image(systemName: device.platform == "mac" ? "desktopcomputer" : "iphone")
                .foregroundColor(.accentColor)
            VStack(alignment: .leading) {
                Text(device.name)
                    .font(.body)
                Text(device.platform.capitalized)
                    .font(.caption2)
                    .foregroundColor(.secondary)
            }
            Spacer()
            if isSelected {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.green)
            }
        }
        .padding()
        .background(isSelected ? Color.accentColor.opacity(0.1) : Color.gray.opacity(0.1))
        .cornerRadius(8)
    }
}
