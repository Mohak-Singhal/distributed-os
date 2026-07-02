import SwiftUI
import UniformTypeIdentifiers

struct TransferView: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var connectionManager: ConnectionManager

    @State private var selectedFiles: [SelectedFile] = []
    @State private var isTargeted = false
    @State private var showFilePicker = false
    @State private var showPhotoPicker = false
    @State private var showTextSheet = false
    @State private var textToSend = ""

    @State private var selectedDeviceID: String?
    @State private var isTransferring = false

    @State private var activeTransfer: ActiveTransferState?
    @State private var transferHistory: [TransferRecord] = []

    @State private var nearbyDevices: [NearbyDevice] = []
    @State private var isScanning = true
    @State private var scanTimer: Timer?
    @State private var showHistory = false

    private let historyKey = "pdos_transfer_history"

    var body: some View {
        VStack(spacing: 0) {
            if let active = activeTransfer {
                transferProgressView(active)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                scrollContent
            }

            if !transferHistory.isEmpty && activeTransfer == nil {
                historyBar
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(
            EffectView(material: .fullScreenUI, blendingMode: .behindWindow)
                .ignoresSafeArea()
        )
        .fileImporter(isPresented: $showFilePicker, allowedContentTypes: [.data, .movie, .image, .audio, .pdf, .text], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result {
                for url in urls { addFile(url) }
            }
        }
        .fileImporter(isPresented: $showPhotoPicker, allowedContentTypes: [.image, .movie], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result {
                for url in urls { addFile(url) }
            }
        }
        .sheet(isPresented: $showTextSheet) {
            textInputSheet
        }
        .onDrop(of: [.fileURL], isTargeted: $isTargeted) { providers in
            handleDrop(providers)
            return true
        }
        .onAppear {
            loadHistory()
            startScanning()
        }
        .onDisappear {
            scanTimer?.invalidate()
        }
        .animation(.spring(duration: 0.4, bounce: 0.2), value: selectedFiles.count)
        .animation(.spring(duration: 0.4, bounce: 0.2), value: selectedDeviceID)
        .animation(.spring(duration: 0.4, bounce: 0.2), value: activeTransfer?.id)
    }

    // MARK: - Scroll Content

    private var scrollContent: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 0) {
                headerSection
                    .padding(.top, 32)
                    .padding(.bottom, 16)

                if selectedFiles.isEmpty {
                    actionButtonsSection
                        .padding(.bottom, 24)
                } else {
                    selectedFilesCard
                        .padding(.bottom, 24)
                }

                nearbyDevicesSection
                    .padding(.bottom, 12)

                if selectedFiles.isEmpty && nearbyDevices.isEmpty {
                    helpSection
                        .padding(.bottom, 20)
                }

                Spacer(minLength: 60)
            }
            .padding(.horizontal, 24)
        }
    }

    // MARK: - Header Section

    private var headerSection: some View {
        VStack(spacing: 4) {
            Text("Send files")
                .font(.system(size: 26, weight: .bold))
                .foregroundColor(.primary)

            Text("Share instantly with nearby devices")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
    }

    // MARK: - Action Buttons (LocalSend-style BigButton)

    private var actionButtonsSection: some View {
        HStack(spacing: 16) {
            actionButton(
                icon: "doc.fill",
                label: "Files",
                action: { showFilePicker = true }
            )
            actionButton(
                icon: "photo.fill",
                label: "Photos",
                action: { showPhotoPicker = true }
            )
            actionButton(
                icon: "text.alignleft",
                label: "Text",
                action: { showTextSheet = true }
            )
        }
    }

    private func actionButton(icon: String, label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: 10) {
                Image(systemName: icon)
                    .font(.system(size: 28))
                    .foregroundColor(.brandCyan)

                Text(label)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundColor(.primary)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 100)
            .background(
                RoundedRectangle(cornerRadius: 16)
                    .fill(Color.primary.opacity(0.04))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .stroke(Color.primary.opacity(0.06), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    // MARK: - Selected Files Card (LocalSend-style)

    private var selectedFilesCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Selection")
                    .font(.headline)
                    .foregroundColor(.primary)

                Spacer()

                Text("\(selectedFiles.count) file\(selectedFiles.count == 1 ? "" : "s")")
                    .font(.subheadline)
                    .foregroundColor(.secondary)

                Text(totalSizeFormatted)
                    .font(.subheadline)
                    .foregroundColor(.secondary)

                Button {
                    withAnimation { selectedFiles.removeAll() }
                } label: {
                    Image(systemName: "xmark")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
                .buttonStyle(.plain)
                .padding(.leading, 4)
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    ForEach(selectedFiles) { file in
                        fileThumbnail(file)
                    }
                }
            }
            .frame(height: 70)

            HStack {
                Spacer()

                Button("Edit") {
                    showFilePicker = true
                }
                .font(.subheadline)
                .foregroundColor(.secondary)

                Text("|")
                    .font(.subheadline)
                    .foregroundColor(.secondary.opacity(0.3))

                Button("Add") {
                    showFilePicker = true
                }
                .font(.subheadline)
                .fontWeight(.semibold)
                .foregroundColor(.brandCyan)
            }
        }
        .padding(16)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(Color.primary.opacity(0.04))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(Color.primary.opacity(0.06), lineWidth: 1)
        )
    }

    private func fileThumbnail(_ file: SelectedFile) -> some View {
        RoundedRectangle(cornerRadius: 10)
            .fill(Color.primary.opacity(0.06))
            .frame(width: 60, height: 60)
            .overlay(
                VStack(spacing: 4) {
                    Image(systemName: file.icon)
                        .font(.system(size: 20))
                        .foregroundColor(.brandCyan)

                    Text(file.url.pathExtension.uppercased())
                        .font(.system(size: 8))
                        .foregroundColor(.secondary)
                        .lineLimit(1)
                }
            )
    }

    private var totalSizeFormatted: String {
        let total = selectedFiles.reduce(0) { $0 + $1.size }
        if total > 1_048_576 { return String(format: "%.1f MB", Double(total) / 1_048_576) }
        if total > 1024 { return String(format: "%.1f KB", Double(total) / 1024) }
        return "\(total) B"
    }

    // MARK: - Text Input Sheet

    private var textInputSheet: some View {
        VStack(spacing: 16) {
            Text("Send Text")
                .font(.headline)

            TextEditor(text: $textToSend)
                .font(.body)
                .frame(height: 150)
                .padding(8)
                .background(
                    RoundedRectangle(cornerRadius: 8)
                        .fill(Color.primary.opacity(0.05))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color.secondary.opacity(0.2), lineWidth: 0.5)
                )

            HStack {
                Button("Cancel") {
                    textToSend = ""
                    showTextSheet = false
                }
                .buttonStyle(.bordered)

                Spacer()

                Button("Send") {
                    if !textToSend.isEmpty {
                        let url = saveTextToFile(textToSend)
                        addFile(url)
                        textToSend = ""
                        showTextSheet = false
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(textToSend.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 360, height: 280)
    }

    private func saveTextToFile(_ text: String) -> URL {
        let tempDir = FileManager.default.temporaryDirectory
        let url = tempDir.appendingPathComponent("text_\(UUID().uuidString.prefix(8)).txt")
        try? text.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    // MARK: - Nearby Devices Section (LocalSend-style)

    private var nearbyDevicesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Nearby devices")
                    .font(.headline)
                    .foregroundColor(.primary)

                if !nearbyDevices.isEmpty {
                    Text("(\(nearbyDevices.count))")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                }

                Spacer()

                HStack(spacing: 4) {
                    Button {
                        withAnimation { nearbyDevices = [] }
                        startScanning()
                    } label: {
                        Image(systemName: "arrow.clockwise")
                            .font(.caption)
                            .foregroundColor(.secondary)
                            .padding(6)
                            .background(Circle().fill(Color.primary.opacity(0.05)))
                    }
                    .buttonStyle(.plain)
                    .help("Scan again")
                    .disabled(isScanning)
                }
            }

            if nearbyDevices.isEmpty {
                VStack(spacing: 12) {
                    if isScanning {
                        ProgressView()
                            .scaleEffect(0.8)
                        Text("Searching for devices...")
                            .font(.subheadline)
                            .foregroundColor(.secondary)
                    } else {
                        Image(systemName: "antenna.radiowaves.left.and.right.slash")
                            .font(.title2)
                            .foregroundColor(.secondary.opacity(0.5))
                        Text("No devices found")
                            .font(.subheadline)
                            .foregroundColor(.secondary)
                        Button("Scan Again") {
                            startScanning()
                        }
                        .font(.subheadline)
                        .foregroundColor(.brandCyan)
                        .buttonStyle(.plain)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 20)
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(nearbyDevices.enumerated()), id: \.element.id) { index, device in
                        deviceRow(device)

                        if index < nearbyDevices.count - 1 {
                            Divider()
                                .padding(.leading, 56)
                        }
                    }
                }
            }
        }
    }

    private func deviceRow(_ device: NearbyDevice) -> some View {
        let isSelected = selectedDeviceID == device.id
        let hasFiles = !selectedFiles.isEmpty

        return Button {
            withAnimation(.spring(duration: 0.3, bounce: 0.15)) {
                if isSelected {
                    selectedDeviceID = nil
                } else {
                    selectedDeviceID = device.id
                    if hasFiles {
                        sendFiles()
                    }
                }
            }
        } label: {
            HStack(spacing: 14) {
                ZStack {
                    Circle()
                        .fill(
                            isSelected
                                ? LinearGradient(colors: [Color.brandCyan, Color.cyan], startPoint: .topLeading, endPoint: .bottomTrailing)
                                : LinearGradient(colors: [Color.primary.opacity(0.06), Color.primary.opacity(0.03)], startPoint: .topLeading, endPoint: .bottomTrailing)
                        )
                        .frame(width: 46, height: 46)

                    Image(systemName: deviceIcon(for: device.platform))
                        .font(.system(size: 20))
                        .foregroundColor(isSelected ? .white : .secondary)

                    if device.isOnline {
                        Circle()
                            .fill(Color.green)
                            .frame(width: 10, height: 10)
                            .offset(x: 16, y: -16)
                    }
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(device.name)
                        .font(.system(size: 15, weight: isSelected ? .semibold : .regular))
                        .foregroundColor(isSelected ? .brandCyan : .primary)
                        .lineLimit(1)

                    if !device.platform.isEmpty {
                        Text(platformLabel(for: device.platform))
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }

                Spacer()

                if isSelected && hasFiles {
                    Image(systemName: "arrow.right.circle.fill")
                        .font(.title3)
                        .foregroundColor(.brandCyan)
                }
            }
            .padding(.vertical, 10)
            .padding(.horizontal, 4)
            .background(isSelected ? Color.brandCyan.opacity(0.06) : Color.clear)
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
    }

    // MARK: - Help Section

    private var helpSection: some View {
        VStack(spacing: 6) {
            Image(systemName: "arrow.up.doc")
                .font(.title2)
                .foregroundColor(.secondary.opacity(0.4))

            Text("Select files, then tap a device to send")
                .font(.subheadline)
                .foregroundColor(.secondary)

            Text("Make sure both devices are on the same network")
                .font(.caption)
                .foregroundColor(.secondary.opacity(0.7))
        }
        .padding(.vertical, 20)
    }

    // MARK: - Transfer Progress (Overlay)

    private func transferProgressView(_ transfer: ActiveTransferState) -> some View {
        VStack(spacing: 24) {
            Spacer()

            if transfer.status == .transferring {
                ZStack {
                    Circle()
                        .stroke(Color.primary.opacity(0.08), lineWidth: 6)
                        .frame(width: 120, height: 120)
                    Circle()
                        .trim(from: 0, to: transfer.progress)
                        .stroke(Color.brandCyan, style: StrokeStyle(lineWidth: 6, lineCap: .round))
                        .frame(width: 120, height: 120)
                        .rotationEffect(.degrees(-90))

                    VStack(spacing: 0) {
                        Text("\(Int(transfer.progress * 100))")
                            .font(.system(size: 34, weight: .bold, design: .monospaced))
                            .foregroundColor(.brandCyan)
                        Text("%")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
            } else {
                ZStack {
                    Circle()
                        .fill(transfer.statusColor.opacity(0.1))
                        .frame(width: 120, height: 120)
                    Image(systemName: transfer.statusIcon)
                        .font(.system(size: 48))
                        .foregroundColor(transfer.statusColor)
                }
            }

            VStack(spacing: 4) {
                Text(transfer.filename)
                    .font(.body)
                    .fontWeight(.medium)
                    .lineLimit(1)

                Text(transfer.formattedProgress)
                    .font(.subheadline)
                    .foregroundColor(.secondary)
            }

            if transfer.status == .transferring {
                Button("Cancel") {
                    withAnimation { cancelTransfer() }
                }
                .font(.subheadline)
                .foregroundColor(.red)
                .buttonStyle(.plain)
                .padding(.top, 4)
            }

            if transfer.status == .completed {
                HStack(spacing: 4) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.caption)
                    Text("Sent")
                }
                .font(.subheadline)
                .foregroundColor(.green)
                .padding(.top, 4)
            }

            if transfer.status == .failed {
                VStack(spacing: 12) {
                    Text(transfer.message)
                        .font(.subheadline)
                        .foregroundColor(.red)

                    Button("Try Again") {
                        retryTransfer()
                    }
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .foregroundColor(.brandCyan)
                    .buttonStyle(.plain)
                }
                .padding(.top, 4)
            }

            Spacer()
        }
        .padding(.horizontal, 40)
    }

    // MARK: - History Bar

    private var historyBar: some View {
        VStack(spacing: 0) {
            Divider()
                .opacity(0.3)

            Button {
                withAnimation(.spring(duration: 0.35, bounce: 0.2)) {
                    showHistory.toggle()
                }
            } label: {
                HStack {
                    Image(systemName: "clock.arrow.circlepath")
                        .font(.caption)
                    Text("History")
                        .font(.caption)
                    Spacer()
                    Text("\(transferHistory.count)")
                        .font(.caption2)
                        .foregroundColor(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(Color.primary.opacity(0.06)))
                }
                .padding(.horizontal, 24)
                .padding(.vertical, 12)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if showHistory {
                VStack(spacing: 0) {
                    let recent = Array(transferHistory.prefix(5))
                    ForEach(recent) { record in
                        HStack(spacing: 10) {
                            Image(systemName: record.statusIcon)
                                .font(.caption)
                                .foregroundColor(record.statusColor)
                                .frame(width: 20)

                            Text(record.filename)
                                .font(.subheadline)
                                .lineLimit(1)

                            Spacer()

                            Text(record.formattedDate)
                                .font(.caption2)
                                .foregroundColor(.secondary)

                            if record.status == .failed {
                                Button("Retry") {
                                    retryHistoryRecord(record)
                                }
                                .font(.caption)
                                .foregroundColor(.brandCyan)
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.horizontal, 24)
                        .padding(.vertical, 8)

                        if record.id != recent.last?.id {
                            Divider()
                                .padding(.leading, 50)
                                .opacity(0.3)
                        }
                    }

                    Divider()
                        .opacity(0.3)

                    Button("Clear", systemImage: "trash") {
                        withAnimation {
                            transferHistory.removeAll()
                            saveHistory()
                            showHistory = false
                        }
                    }
                    .font(.caption2)
                    .foregroundColor(.secondary)
                    .buttonStyle(.plain)
                    .padding(.vertical, 8)
                }
                .padding(.bottom, 8)
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .background(.ultraThinMaterial)
    }

    // MARK: - Transfer Logic

    private func addFile(_ url: URL) {
        guard !selectedFiles.contains(where: { $0.url == url }) else { return }
        let attrs = try? FileManager.default.attributesOfItem(atPath: url.path)
        let size = attrs?[.size] as? UInt64 ?? 0
        withAnimation { selectedFiles.append(SelectedFile(url: url, size: size)) }
    }

    private func handleDrop(_ providers: [NSItemProvider]) {
        for provider in providers {
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, _ in
                guard let data = item as? Data,
                      let url = URL(dataRepresentation: data, relativeTo: nil) else { return }
                DispatchQueue.main.async { self.addFile(url) }
            }
        }
    }

    private func sendFiles() {
        guard let deviceID = selectedDeviceID, !selectedFiles.isEmpty else { return }
        isTransferring = true

        let file = selectedFiles[0]
        activeTransfer = ActiveTransferState(
            id: UUID().uuidString,
            filename: file.url.lastPathComponent,
            totalSize: file.size,
            status: .transferring
        )

        FileTransferService.sendFile(url: file.url, deviceID: deviceID) { success, error in
            DispatchQueue.main.async {
                self.isTransferring = false
                if success {
                    self.activeTransfer?.status = .completed
                    self.activeTransfer?.progress = 1.0

                    let record = TransferRecord(
                        id: UUID().uuidString,
                        filename: file.url.lastPathComponent,
                        fileSize: file.size,
                        date: Date(),
                        status: .completed,
                        deviceID: deviceID
                    )
                    self.transferHistory.insert(record, at: 0)
                    self.saveHistory()

                    DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                        withAnimation {
                            self.activeTransfer = nil
                            self.selectedFiles.removeAll { $0.id == file.id }
                            self.selectedDeviceID = nil
                        }
                    }
                } else {
                    self.activeTransfer?.status = .failed
                    self.activeTransfer?.message = error ?? "Could not send file"

                    let record = TransferRecord(
                        id: UUID().uuidString,
                        filename: file.url.lastPathComponent,
                        fileSize: file.size,
                        date: Date(),
                        status: .failed,
                        deviceID: deviceID,
                        error: error
                    )
                    self.transferHistory.insert(record, at: 0)
                    self.saveHistory()
                }
            }
        }

        animateProgress()
    }

    private func animateProgress() {
        guard activeTransfer?.status == .transferring else { return }
        withAnimation(.linear(duration: 0.3)) {
            activeTransfer?.progress = min(1.0, (activeTransfer?.progress ?? 0) + 0.05)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { animateProgress() }
    }

    private func cancelTransfer() {
        activeTransfer = nil
        isTransferring = false
    }

    private func retryTransfer() {
        guard let failed = activeTransfer, failed.status == .failed else { return }
        activeTransfer?.status = .transferring
        activeTransfer?.progress = 0
        activeTransfer?.message = ""
        sendFiles()
    }

    private func retryHistoryRecord(_ record: TransferRecord) {
        selectedDeviceID = record.deviceID
        if !selectedFiles.isEmpty {
            sendFiles()
        }
    }

    // MARK: - Scanning

    private func startScanning() {
        isScanning = true
        loadNearbyDevices()
        scanTimer?.invalidate()
        scanTimer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { _ in
            loadNearbyDevices()
        }
    }

    private func loadNearbyDevices() {
        let serviceDevices = FileTransferService.parseKnownDevices()
        let connectedNodes = NodeService.shared.nodes.filter { $0.status == "online" || $0.status == "device" }

        var all: [NearbyDevice] = []

        for d in serviceDevices {
            if !all.contains(where: { $0.id == d.id }) {
                all.append(NearbyDevice(id: d.id, name: d.name, platform: d.platform, isOnline: true))
            }
        }

        for node in connectedNodes {
            if !all.contains(where: { $0.id == node.nodeId }) {
                all.append(NearbyDevice(id: node.nodeId, name: node.displayName, platform: node.platform, isOnline: true))
            }
        }

        let trusted = DevicePersistence.knownDevices
        for d in trusted {
            if !all.contains(where: { $0.id == d.id }) {
                all.append(NearbyDevice(id: d.id, name: d.name, platform: d.platform, isOnline: false))
            }
        }

        withAnimation { nearbyDevices = all }
        isScanning = false
    }

    // MARK: - Persistence

    private func loadHistory() {
        guard let data = UserDefaults.standard.data(forKey: historyKey),
              let decoded = try? JSONDecoder().decode([TransferRecord].self, from: data) else { return }
        transferHistory = decoded
    }

    private func saveHistory() {
        guard let encoded = try? JSONEncoder().encode(transferHistory) else { return }
        UserDefaults.standard.set(encoded, forKey: historyKey)
    }

    private func deviceIcon(for platform: String) -> String {
        switch platform.lowercased() {
        case "macos", "mac": return "laptopcomputer"
        case "android": return "iphone.gen2"
        case "linux": return "desktopcomputer"
        case "windows": return "pc"
        case "relay": return "server.rack"
        default: return "antenna.radiowaves.left.and.right"
        }
    }

    private func platformLabel(for platform: String) -> String {
        switch platform.lowercased() {
        case "macos", "mac": return ""
        case "android": return ""
        case "linux": return ""
        case "windows": return ""
        default: return ""
        }
    }
}

// MARK: - Supporting Types

struct SelectedFile: Identifiable, Equatable {
    let id = UUID()
    let url: URL
    let size: UInt64

    var formattedSize: String {
        if size > 1_048_576 { return String(format: "%.1f MB", Double(size) / 1_048_576) }
        if size > 1024 { return String(format: "%.1f KB", Double(size) / 1024) }
        return "\(size) B"
    }

    var icon: String {
        let ext = url.pathExtension.lowercased()
        switch ext {
        case "jpg", "jpeg", "png", "gif", "webp", "heic": return "photo"
        case "mp4", "mov", "avi", "mkv": return "film"
        case "mp3", "wav", "aac", "flac": return "music.note"
        case "pdf": return "doc.richtext"
        case "zip", "tar", "gz": return "archivebox"
        case "doc", "docx": return "doc.text"
        default: return "doc"
        }
    }
}

struct NearbyDevice: Identifiable {
    let id: String
    let name: String
    let platform: String
    let isOnline: Bool
}

enum TransferStatus: String, Codable {
    case transferring
    case completed
    case failed
}

struct ActiveTransferState: Identifiable {
    let id: String
    let filename: String
    let totalSize: UInt64
    var status: TransferStatus = .transferring
    var progress: Double = 0
    var message: String = ""
    var currentSpeed: Double = 0

    var formattedProgress: String {
        let sent = UInt64(progress * Double(totalSize))
        if sent > 1_048_576 { return String(format: "%.1f / %.1f MB", Double(sent) / 1_048_576, Double(totalSize) / 1_048_576) }
        if sent > 1024 { return String(format: "%.1f / %.1f KB", Double(sent) / 1024, Double(totalSize) / 1024) }
        return "\(sent) / \(totalSize) B"
    }

    var statusIcon: String {
        switch status {
        case .transferring: return "arrow.up.circle.fill"
        case .completed: return "checkmark.circle.fill"
        case .failed: return "xmark.circle.fill"
        }
    }

    var statusColor: Color {
        switch status {
        case .transferring: return .brandCyan
        case .completed: return .green
        case .failed: return .red
        }
    }
}

struct TransferRecord: Codable, Identifiable {
    let id: String
    let filename: String
    let fileSize: UInt64
    let date: Date
    let status: TransferStatus
    let deviceID: String
    var error: String?

    var formattedSize: String {
        if fileSize > 1_048_576 { return String(format: "%.1f MB", Double(fileSize) / 1_048_576) }
        if fileSize > 1024 { return String(format: "%.1f KB", Double(fileSize) / 1024) }
        return "\(fileSize) B"
    }

    var formattedDate: String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    var statusIcon: String {
        switch status {
        case .transferring: return "arrow.up.circle"
        case .completed: return "checkmark.circle.fill"
        case .failed: return "xmark.circle"
        }
    }

    var statusColor: Color {
        switch status {
        case .transferring: return .brandCyan
        case .completed: return .green
        case .failed: return .red
        }
    }
}

