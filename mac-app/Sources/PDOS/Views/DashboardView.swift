import SwiftUI

struct DashboardView: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var connectionManager: ConnectionManager

    @State private var showDebugMetrics = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header
                if backend.metrics != nil || backend.healthScore != nil {
                    primaryGrid
                    secondaryGrid
                    if showDebugMetrics { tertiarySection }
                } else {
                    emptyState
                }
            }
            .padding(Spacing.xxl)
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(alignment: .center) {
            Text("System Dashboard")
                .font(.system(size: 32, weight: .bold, design: .default))
                .foregroundColor(.primary)

            Spacer()

            if connectionManager.isConnecting {
                Capsule()
                    .fill(.ultraThinMaterial)
                    .frame(height: 28)
                    .overlay(
                        HStack(spacing: 6) {
                            ProgressView()
                                .progressViewStyle(.circular)
                                .scaleEffect(0.5)
                                .frame(width: 12, height: 12)
                            Text("Connecting...")
                                .font(.caption)
                                .foregroundColor(.secondary)
                        }
                    )
                    .fixedSize()
            }

            Button { showDebugMetrics.toggle() } label: {
                Image(systemName: "gearshape")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
            .buttonStyle(.plain)
            .help("Toggle debug metrics")
        }
        .padding(.bottom, Spacing.xs)
    }

    // MARK: - Empty State (Living)

    private var emptyState: some View {
        VStack(spacing: 20) {
            Spacer().frame(height: 40)
            breathingRings
            emptyStateText
            emptyStateButton
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    @State private var breathScale: CGFloat = 1
    @State private var breathOpacity: Double = 0.6

    private var breathingRings: some View {
        let ring1 = Circle().stroke(Color.cyan.opacity(0.15), lineWidth: 2)
            .frame(width: 80, height: 80)
        let ring2 = Circle().stroke(Color.cyan.opacity(0.11), lineWidth: 2)
            .frame(width: 104, height: 104)
        let ring3 = Circle().stroke(Color.cyan.opacity(0.07), lineWidth: 2)
            .frame(width: 128, height: 128)
        let icon = Image(systemName: "chart.line.downtrend.xyaxis")
            .font(.system(size: 36))
            .foregroundColor(.secondary.opacity(0.3))
        return ZStack {
            ring1.scaleEffect(breathScale).opacity(breathOpacity)
            ring2.scaleEffect(breathScale).opacity(breathOpacity)
            ring3.scaleEffect(breathScale).opacity(breathOpacity)
            icon
        }
    }

    private var emptyStateText: some View {
        VStack(spacing: 8) {
            Text("Your live dashboard is one click away")
                .font(.title3)
                .foregroundColor(.primary)
            Text("Start the relay to see system metrics, health scores, and transfer data stream in real time.")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 320)
        }
    }

    private var emptyStateButton: some View {
        Button { connectionManager.startRelay(backend: backend) } label: {
            Label("Start Relay", systemImage: "play.fill")
                .frame(maxWidth: 200)
        }
        .buttonStyle(.borderedProminent)
        .controlSize(.large)
        .disabled(connectionManager.isConnecting || backend.relayRunning)
    }

    // MARK: - Primary Grid (Full Width)

    private var primaryGrid: some View {
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 16) {
            healthCard
                .gridCellColumns(2)
            systemCard
                .gridCellColumns(2)
        }
    }

    private var healthCard: some View {
        GlassCard {
            HStack(spacing: 0) {
                if let h = backend.healthScore {
                    // Radial gauge
                    ZStack {
                        Circle()
                            .stroke(scoreColor(h.overall).opacity(0.15), lineWidth: 8)
                            .frame(width: 88, height: 88)
                        Circle()
                            .trim(from: 0, to: animatedHealthPct)
                            .stroke(scoreColor(h.overall), style: StrokeStyle(lineWidth: 8, lineCap: .round))
                            .frame(width: 88, height: 88)
                            .rotationEffect(.degrees(-90))
                        VStack(spacing: 0) {
                            Text("\(Int(h.overall))")
                                .font(.system(size: 28, weight: .bold))
                                .foregroundColor(scoreColor(h.overall))
                            Text("/100")
                                .font(.caption2)
                                .foregroundColor(.secondary)
                        }
                    }

                    Spacer().frame(width: 24)

                    VStack(alignment: .leading, spacing: 10) {
                        Text("Health Score")
                            .font(.headline)
                        HStack(spacing: 16) {
                            compactBar("CPU", h.cpu / 100, color: scoreColor(h.cpu))
                            compactBar("Disk", h.disk / 100, color: scoreColor(h.disk))
                            compactBar("Network", h.network / 100, color: scoreColor(h.network))
                            compactBar("Integrity", h.integrity / 100, color: scoreColor(h.integrity))
                            compactBar("Recovery", h.recovery / 100, color: scoreColor(h.recovery))
                        }
                    }
                }
            }
            .frame(minHeight: 100)
        }
    }

    @State private var animatedHealthPct: CGFloat = 0

    private var systemCard: some View {
        GlassCard {
            HStack(spacing: 24) {
                if let m = backend.metrics {
                    metricTile("CPU", value: m.cpu_usage ?? "--", icon: "cpu")
                    Divider().frame(height: 48)
                    metricTile("Memory", value: "\(m.memory_mb ?? "0") MB", icon: "memorychip")
                    Divider().frame(height: 48)
                    VStack(spacing: 4) {
                        Image(systemName: "arrow.down.arrow.up")
                            .font(.caption).foregroundColor(.secondary)
                        HStack(spacing: 2) {
                            Image(systemName: "arrow.down").font(.caption2).foregroundColor(.brandCyan)
                            Text(m.network_rx_mbps ?? "0")
                        }
                        .font(.subheadline).fontWeight(.medium)
                        HStack(spacing: 2) {
                            Image(systemName: "arrow.up").font(.caption2).foregroundColor(.orange)
                            Text(m.network_tx_mbps ?? "0")
                        }
                        .font(.subheadline).fontWeight(.medium)
                        Text("Mbps").font(.caption2).foregroundColor(.secondary)
                    }
                } else {
                    Text("No data").foregroundColor(.secondary)
                }
            }
            .frame(minHeight: 72)
        }
    }

    // MARK: - Secondary Grid (Half Width)

    private var secondaryGrid: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 260))], spacing: 16) {
            diskCard
            thermalCard
            bottleneckCard
        }
    }

    private var diskCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Disks", systemImage: "externaldrive").font(.headline)
                if let disks = backend.metrics?.disks {
                    ForEach(disks) { d in
                        let used = d.total_gb - d.available_gb
                        let pct = d.total_gb > 0 ? used / d.total_gb : 0
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(d.name).font(.subheadline).lineLimit(1)
                                Spacer()
                                Text("\(String(format: "%.0f", pct * 100))%")
                                    .font(.caption).foregroundColor(.secondary)
                            }
                            ProgressView(value: pct)
                                .tint(pct > 0.9 ? Color.red : pct > 0.7 ? Color.orange : Color.brandCyan)
                            Text("\(String(format: "%.1f", d.available_gb)) GB free")
                                .font(.caption).foregroundColor(.secondary)
                        }
                        if d.name != disks.last?.name { Divider() }
                    }
                }
            }
        }
    }

    private var thermalCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Thermal", systemImage: "thermometer").font(.headline)
                if let t = backend.metrics?.thermal {
                    HStack(spacing: 16) {
                        thermalMetric("State", t.thermal_state ?? "--")
                        thermalMetric("CPU", t.cpu_temp_c.map { "\(String(format: "%.0f", $0))°C" } ?? "--")
                        thermalMetric("Battery", t.battery_pct.map { "\(Int($0))%" } ?? "--")
                        thermalMetric("Fan", t.fan_rpm.map { "\(Int($0))" } ?? "--")
                    }
                }
            }
        }
    }

    private var bottleneckCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 8) {
                Label("Bottleneck", systemImage: "exclamationmark.triangle").font(.headline)
                if let b = backend.bottleneck, let name = b.bottleneck, !name.isEmpty {
                    Text(name).font(.subheadline).bold()
                    if let rec = b.recommendation, !rec.isEmpty {
                        Text(rec).font(.caption).foregroundColor(.secondary)
                    }
                } else {
                    HStack {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundColor(.green).font(.caption)
                        Text("No bottleneck detected")
                            .font(.subheadline).foregroundColor(.secondary)
                    }
                }
            }
        }
    }

    // MARK: - Debug Section (Collapsible)

    private var tertiarySection: some View {
        VStack(spacing: 12) {
            DisclosureGroup("Protocol & Buffer Stats") {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 260))], spacing: 16) {
                    protocolsCard
                    bufferCard
                }
            }
            .font(.subheadline)
        }
    }

    private var protocolsCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 4) {
                Label("Protocol", systemImage: "point.3.connected.trianglepath.dotted").font(.subheadline)
                if let p = backend.metrics?.protocol_stats {
                    ForEach(nonZeroProtocolStats(p), id: \.0) { label, val in
                        HStack {
                            Text(label).foregroundColor(.secondary).font(.caption)
                            Spacer()
                            Text("\(val)").font(.caption).fontWeight(.medium)
                        }
                    }
                    if nonZeroProtocolStats(p).isEmpty {
                        Text("No activity yet").foregroundColor(.secondary).font(.caption)
                    }
                }
            }
        }
    }

    private var bufferCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 4) {
                Label("Buffer", systemImage: "rectangle.split.2x2").font(.subheadline)
                if let b = backend.bufferAnalysis {
                    row("Read", "\(b.read_buffer_kb) KB")
                    row("Write", "\(b.write_buffer_kb) KB")
                    row("Avg Queue", String(format: "%.1f", b.average_queue_depth))
                }
            }
        }
    }

    // MARK: - Helpers

    private func nonZeroProtocolStats(_ p: ProtocolStats) -> [(String, Int)] {
        [
            ("Completed", p.completed_transfers),
            ("Failed", p.failed_transfers),
            ("Auth", p.auth_requests),
            ("Discovery", p.discovery_packets),
            ("Resumes", p.resume_requests),
            ("Range", p.range_requests),
        ].filter { $0.1 > 0 }
    }

    private func metricTile(_ label: String, value: String, icon: String) -> some View {
        VStack(spacing: 4) {
            Image(systemName: icon)
                .font(.caption)
                .foregroundColor(.secondary)
            Text(value)
                .font(.subheadline)
                .fontWeight(.medium)
            Text(label)
                .font(.caption2)
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity)
    }

    private func compactBar(_ label: String, _ pct: Double, color: Color) -> some View {
        VStack(spacing: 2) {
            Text(label).font(.system(size: 9)).foregroundColor(.secondary)
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(.quaternary)
                    .frame(width: 36, height: 4)
                RoundedRectangle(cornerRadius: 2)
                    .fill(color)
                    .frame(width: 36 * CGFloat(max(0, min(1, pct))), height: 4)
            }
        }
    }

    private func thermalMetric(_ label: String, _ value: String) -> some View {
        VStack(spacing: 2) {
            Text(value).font(.subheadline).fontWeight(.medium)
            Text(label).font(.caption2).foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity)
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).foregroundColor(.secondary).font(.caption)
            Spacer()
            Text(value).font(.caption).fontWeight(.medium)
        }
    }

    private func scoreColor(_ s: Double) -> Color {
        s >= 80 ? .green : s >= 50 ? .orange : .red
    }
}
