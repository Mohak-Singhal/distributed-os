import SwiftUI
import UniformTypeIdentifiers

struct FileDropHandler: ViewModifier {
    @EnvironmentObject var backend: BackendService
    @State private var isDragOver = false
    @State private var droppedFiles: [URL] = []
    @State private var showDevicePicker = false
    @State private var transferStatus: String?

    func body(content: Content) -> some View {
        content
            .overlay {
                if isDragOver {
                    DropOverlayView(status: transferStatus)
                }
            }
            .onDrop(of: [.fileURL], isTargeted: $isDragOver) { providers in
                handleDrop(providers: providers)
                return true
            }
            .sheet(isPresented: $showDevicePicker) {
                DevicePickerSheet(
                    files: droppedFiles,
                    isPresented: $showDevicePicker,
                    onSend: { deviceID in
                        sendDroppedFiles(deviceID: deviceID)
                    }
                )
            }
    }

    private func handleDrop(providers: [NSItemProvider]) {
        droppedFiles = []
        transferStatus = nil
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

    private func sendDroppedFiles(deviceID: String) {
        guard !deviceID.isEmpty, !droppedFiles.isEmpty else { return }
        transferStatus = "Sending \(droppedFiles.count) file(s)..."

        FileTransferService.sendFiles(droppedFiles, deviceID: deviceID) { success, fail, _ in
            self.transferStatus = "Sent \(success) file(s)"
            Task { await self.backend.refreshAll() }
        }
    }
}

struct DropOverlayView: View {
    let status: String?

    var body: some View {
        RoundedRectangle(cornerRadius: Radius.card)
            .stroke(Color.cyan, lineWidth: 3)
            .background(Color.cyan.opacity(0.1))
            .overlay(
                VStack(spacing: 12) {
                    Image(systemName: "arrow.down.doc.fill")
                        .font(.system(size: 48))
                        .foregroundColor(.cyan)
                    Text("Drop files to send")
                        .font(.title2)
                        .bold()
                        .foregroundColor(.cyan)
                    if let status = status {
                        Text(status)
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
            )
            .ignoresSafeArea()
            .allowsHitTesting(false)
    }
}

extension View {
    func onFileDrop(backend: BackendService) -> some View {
        modifier(FileDropHandler())
    }
}
