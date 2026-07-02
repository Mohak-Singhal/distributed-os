import SwiftUI

struct NetworkView: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var hotspotDetector: HotspotDetector

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Network")
                    .font(.largeTitle)
                    .bold()

                LazyVGrid(columns: [GridItem(.adaptive(minimum: 380))], spacing: 16) {
                    interfacesCard
                    wifiCard
                    devicesCard
                    pathsCard
                }
            }
            .padding(24)
        }
    }

    var interfacesCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Interfaces", systemImage: "cable.connector").font(.headline)
                Divider()
                if let ifs = backend.metrics?.interfaces {
                    ForEach(ifs.filter { $0.rx > 0 || $0.tx > 0 || $0.name == "en0" || $0.name == "en1" }) { i in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(i.name).font(.subheadline).bold()
                                Text(i.mac).font(.caption).foregroundColor(.secondary)
                            }
                            Spacer()
                            VStack(alignment: .trailing) {
                                Text("▼ \(bytes(i.rx))").font(.caption)
                                Text("▲ \(bytes(i.tx))").font(.caption)
                            }
                        }
                        Divider()
                    }
                } else {
                    Text("No data").foregroundColor(.secondary)
                }
            }
        }
    }

    var wifiCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("WiFi / Hotspot", systemImage: "wifi").font(.headline)
                Divider()
                row("Type", hotspotDetector.networkType.rawValue)
                row("SSID", hotspotDetector.ssid ?? "Not connected")
                row("Gateway", hotspotDetector.gatewayIP ?? "--")
                row("Interface", hotspotDetector.interface ?? "--")
            }
        }
    }

    var devicesCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Discovered Devices", systemImage: "antenna.radiowaves.left.and.right").font(.headline)
                Divider()
                Text("mDNS scanning active on _pdos._tcp.local")
                    .font(.caption).foregroundColor(.secondary)
                Text("Devices will appear when connected to relay")
                    .foregroundColor(.secondary)

                Divider()

                let knownRelays = DevicePersistence.knownRelays
                if !knownRelays.isEmpty {
                    Text("Known Relays").font(.subheadline).bold()
                    ForEach(knownRelays.prefix(5)) { relay in
                        HStack {
                            Circle().fill(.green).frame(width: 6, height: 6)
                            Text(relay.name)
                                .font(.subheadline)
                            Spacer()
                            Text(relay.url)
                                .font(.caption)
                                .foregroundColor(.secondary)
                        }
                    }
                }
            }
        }
    }

    var pathsCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Network Paths", systemImage: "point.3.connected.trianglepath.dotted").font(.headline)
                Divider()
                if let m = backend.metrics {
                    let rx = m.network_rx_mbps ?? "0"
                    let tx = m.network_tx_mbps ?? "0"
                    row("Download", "\(rx) Mbps")
                    row("Upload", "\(tx) Mbps")
                    if let ifs = m.interfaces {
                        let active = ifs.filter { $0.rx > 0 || $0.tx > 0 }
                        row("Active Interfaces", "\(active.count)")
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

    func bytes(_ b: Int) -> String {
        if b > 1_048_576 { return String(format: "%.1f MB", Double(b) / 1_048_576) }
        if b > 1024 { return String(format: "%.1f KB", Double(b) / 1024) }
        return "\(b) B"
    }
}
