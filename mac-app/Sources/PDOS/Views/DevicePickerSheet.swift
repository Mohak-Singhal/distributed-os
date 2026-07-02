import SwiftUI

struct DevicePickerSheet: View {
    let files: [URL]
    @Binding var isPresented: Bool
    let onSend: (String) -> Void

    @State private var knownDevices: [Device] = []
    @State private var isLoading = true
    @State private var searchText = ""

    var body: some View {
        VStack(spacing: 0) {
            headerSection
            Divider()
            filePreviewSection
            Divider()
            deviceListSection
            Divider()
            actionButtons
        }
        .frame(width: 400, height: 440)
        .onAppear { loadDevices() }
    }

    private var headerSection: some View {
        VStack(spacing: 4) {
            Text("Send Files")
                .font(.headline)
            Text("Choose a destination device")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .padding(.vertical, 16)
    }

    private var filePreviewSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("\(files.count) file\(files.count == 1 ? "" : "s")", systemImage: "doc.on.doc")
                .font(.caption)
                .foregroundColor(.secondary)
                .padding(.horizontal, 16)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(files, id: \.self) { file in
                        HStack(spacing: 4) {
                            Image(systemName: fileIcon(for: file.pathExtension))
                                .font(.caption)
                                .foregroundColor(.secondary)
                            Text(file.lastPathComponent)
                                .font(.caption)
                                .lineLimit(1)
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(
                            RoundedRectangle(cornerRadius: 6)
                                .fill(Color.primary.opacity(0.06))
                        )
                    }
                }
                .padding(.horizontal, 16)
            }
            .frame(height: 28)
        }
        .padding(.vertical, 12)
    }

    private var deviceListSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundColor(.secondary)
                    .font(.caption)
                TextField("Search devices", text: $searchText)
                    .textFieldStyle(.plain)
                    .font(.subheadline)
                if !searchText.isEmpty {
                    Button { searchText = "" } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(8)
            .background(
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.primary.opacity(0.05))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .strokeBorder(Color.secondary.opacity(0.15), lineWidth: 0.5)
                    )
            )
            .padding(.horizontal, 16)

            if isLoading {
                HStack {
                    Spacer()
                    ProgressView("Scanning for devices...")
                        .controlSize(.small)
                        .padding(.vertical, 24)
                    Spacer()
                }
            } else if filteredDevices.isEmpty {
                VStack(spacing: 6) {
                    Image(systemName: "antenna.radiowaves.left.and.right.slash")
                        .font(.title2)
                        .foregroundColor(.secondary)
                    Text("No devices found")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                    Button("Scan Again") { loadDevices() }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 24)
            } else {
                ScrollView {
                    VStack(spacing: 4) {
                        ForEach(filteredDevices) { device in
                            Button(action: {
                                onSend(device.id)
                                isPresented = false
                            }) {
                                HStack(spacing: 12) {
                                    Text(String(device.name.prefix(1)).uppercased())
                                        .font(.headline)
                                        .foregroundColor(.white)
                                        .frame(width: 36, height: 36)
                                        .background(
                                            Circle()
                                                .fill(LinearGradient(
                                                    colors: [.brandCyan, .cyan],
                                                    startPoint: .topLeading,
                                                    endPoint: .bottomTrailing
                                                ))
                                        )

                                    Text(device.name)
                                        .font(.subheadline)
                                        .foregroundColor(.primary)

                                    Spacer()

                                    Image(systemName: "arrow.right.circle.fill")
                                        .foregroundColor(.brandCyan)
                                        .font(.caption)
                                }
                                .padding(.horizontal, 12)
                                .padding(.vertical, 10)
                                .background(
                                    RoundedRectangle(cornerRadius: 10)
                                        .fill(Color.primary.opacity(0.03))
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 16)
                }
            }
        }
        .padding(.vertical, 12)
    }

    private var actionButtons: some View {
        HStack {
            Button("Cancel") { isPresented = false }
                .keyboardShortcut(.cancelAction)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private var filteredDevices: [Device] {
        if searchText.isEmpty { return knownDevices }
        return knownDevices.filter {
            $0.name.localizedCaseInsensitiveContains(searchText)
        }
    }

    private func loadDevices() {
        isLoading = true
        FileTransferService.listDevices { devices in
            knownDevices = devices
            isLoading = false
        }
    }

    private func fileIcon(for ext: String) -> String {
        switch ext.lowercased() {
        case "jpg", "jpeg", "png", "gif", "webp", "heic": return "photo"
        case "mp4", "mov", "avi", "mkv": return "film"
        case "mp3", "wav", "aac", "flac": return "music.note"
        case "pdf": return "doc.richtext"
        case "zip", "tar", "gz": return "archivebox"
        default: return "doc"
        }
    }
}
