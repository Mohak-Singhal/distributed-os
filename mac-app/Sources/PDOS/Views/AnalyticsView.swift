import SwiftUI

struct AnalyticsView: View {
    @EnvironmentObject var backend: BackendService
    @State private var protocolStats: ProtocolStats?
    @State private var forecastPath = "/tmp"
    @State private var forecastSize = "1048576"
    @State private var forecastResult: StorageForecast?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Analytics")
                    .font(.largeTitle)
                    .bold()

                LazyVGrid(columns: [GridItem(.adaptive(minimum: 340))], spacing: 16) {
                    protocolCard
                    compressionCard
                    forecastCard
                    networkCard
                }
            }
            .padding(24)
        }
        .task {
            protocolStats = await backend.fetchProtocolStats()
        }
    }

    var protocolCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Protocol Stats", systemImage: "network").font(.headline)
                Divider()
                if let p = protocolStats {
                    Group {
                        row("Auth Requests", "\(p.auth_requests)")
                        row("Transfer Requests", "\(p.transfer_requests)")
                        row("Completed", "\(p.completed_transfers)")
                        row("Failed", "\(p.failed_transfers)")
                        row("Cancelled", "\(p.cancelled_transfers)")
                        row("Resume Requests", "\(p.resume_requests)")
                        row("Range Requests", "\(p.range_requests)")
                        row("Discovery Packets", "\(p.discovery_packets)")
                        row("TLS Handshakes", "\(p.tls_handshakes)")
                    }
                } else {
                    Text("No data").foregroundColor(.secondary)
                }
            }
        }
    }

    var compressionCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Compression Summary", systemImage: "square.and.arrow.down").font(.headline)
                Divider()
                let sessions = backend.history.filter { $0.compression_ratio != nil }
                if sessions.isEmpty {
                    Text("No compression data yet").foregroundColor(.secondary)
                } else {
                    let avgRatio = sessions.compactMap { $0.compression_ratio }.reduce(0, +) / Double(max(1, sessions.count))
                    let totalOriginal = sessions.compactMap { $0.file_size }.reduce(0, +)
                    let totalCompressed = sessions.compactMap { $0.compressed_size }.reduce(0, +)
                    let totalSaved = sessions.compactMap { $0.bandwidth_saved }.reduce(0, +)
                    let totalTime = sessions.compactMap { $0.compression_time_ms }.reduce(0, +)

                    row("Files Compressed", "\(sessions.count)")
                    row("Avg Ratio", String(format: "%.2f", avgRatio))
                    row("Total Original", bytesString(totalOriginal))
                    row("Total Compressed", bytesString(totalCompressed))
                    row("Bandwidth Saved", bytesString(totalSaved))
                    row("Total Comp Time", "\(totalTime) ms")
                    row("Avg Speed", sessions.compactMap { $0.transfer_speed_mbps }.count > 0
                        ? String(format: "%.1f Mbps", sessions.compactMap { $0.transfer_speed_mbps }.reduce(0, +) / Double(sessions.count))
                        : "N/A")
                }
            }
        }
    }

    var forecastCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Storage Forecast", systemImage: "chart.bar").font(.headline)
                Divider()
                HStack {
                    TextField("Path", text: $forecastPath).textFieldStyle(.roundedBorder)
                    TextField("Size (bytes)", text: $forecastSize).textFieldStyle(.roundedBorder)
                    Button("Check") {
                        Task {
                            forecastResult = await backend.fetchStorageForecast(
                                path: forecastPath, size: Int(forecastSize) ?? 1048576
                            )
                        }
                    }
                }
                if let f = forecastResult {
                    Divider()
                    row("File Size", bytesString(UInt64(f.file_size_gb * 1_073_741_824)))
                    row("Free Space", String(format: "%.2f GB", f.free_gb))
                    row("After Transfer", String(format: "%.2f GB", f.remaining_gb))
                    row("Enough Space", f.enough_space ? "Yes" : "No")
                }
            }
        }
    }

    var networkCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Network Paths", systemImage: "point.connected.points").font(.headline)
                Divider()
                if let netIf = backend.metrics?.interfaces {
                    ForEach(netIf.filter { $0.rx > 0 || $0.tx > 0 }) { i in
                        row(i.name, "▼\(i.rx) ▲\(i.tx)")
                    }
                } else {
                    Text("No data").foregroundColor(.secondary)
                }
            }
        }
    }

    func row(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).foregroundColor(.secondary)
            Spacer()
            Text(value).fontWeight(.medium)
        }
        .font(.subheadline)
    }

    func bytesString(_ bytes: UInt64) -> String {
        if bytes > 1_073_741_824 { return String(format: "%.2f GB", Double(bytes) / 1_073_741_824) }
        if bytes > 1_048_576 { return String(format: "%.1f MB", Double(bytes) / 1_048_576) }
        if bytes > 1024 { return String(format: "%.1f KB", Double(bytes) / 1024) }
        return "\(bytes) B"
    }
}
