import SwiftUI

struct Theme {
    static let background = Color(hex: "#0A0A0F")       // Dark Background
    static let sidebar = Color(hex: "#0F0F17")          // Dark Surface
    static let card = Color(hex: "#1A1A28")             // Dark Surface Card
    static let accent = Color(hex: "#00E5FF")           // brandCyan (neon blue)
    static let textSecondary = Color(hex: "#9E9EB0")    // darkTextSecondary
    static let border = Color(hex: "#2A2A3A")           // darkDivider
    static let green = Color(hex: "#00E676")            // success
    static let orange = Color(hex: "#FFAB00")           // warning
    static let red = Color(hex: "#FF1744")              // error
    static let blue = Color(hex: "#40C4FF")             // info
    static let purple = Color(hex: "#BB86FC")           // brandIndigo
}

extension Color {
    init(hex: String) {
        let hex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch hex.count {
        case 3: (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default: (a, r, g, b) = (255, 0, 0, 0)
        }
        self.init(.sRGB, red: Double(r)/255, green: Double(g)/255, blue: Double(b)/255, opacity: Double(a)/255)
    }
}

// MARK: - Main View

struct ContentView: View {
    @StateObject private var vm = AppViewModel()
    @State private var selectedTab: String = "receive"
    @State private var selectedTransferId: String? = nil

    var body: some View {
        HStack(spacing: 0) {
            sidebar
            mainContent
        }
        .frame(minWidth: 1100, minHeight: 700)
        .background(Theme.background)
        .onAppear {
            vm.startTelemetryPolling()
            if !vm.isScanning && vm.selectedNode == nil {
                vm.toggleScan()
            }
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("PDOS").font(.title).bold().foregroundColor(.white).padding(.leading, 16).padding(.top, 32)
            
            Text("CORE").font(.caption).foregroundColor(Theme.textSecondary).padding(.leading, 16).padding(.top, 24)
            SidebarButton(title: "Receive", icon: "square.and.arrow.down.fill", tab: "receive")
            SidebarButton(title: "Send", icon: "paperplane.fill", tab: "send")
            SidebarButton(title: "Settings", icon: "gearshape.fill", tab: "settings")

            Text("DIAGNOSTICS").font(.caption).foregroundColor(Theme.textSecondary).padding(.leading, 16).padding(.top, 24)
            SidebarButton(title: "Monitor", icon: "chart.xyaxis.line", tab: "monitor")
            SidebarButton(title: "Network", icon: "network", tab: "network")
            SidebarButton(title: "Health", icon: "heart.text.clipboard", tab: "health")

            Spacer()
        }
        .frame(width: 200)
        .background(Theme.sidebar)
    }

    private func SidebarButton(title: String, icon: String, tab: String) -> some View {
        Button(action: { selectedTab = tab }) {
            HStack(spacing: 12) {
                Image(systemName: icon).frame(width: 20)
                Text(title).fontWeight(.semibold)
                Spacer()
            }
            .foregroundColor(selectedTab == tab ? .black : .white)
            .padding(.vertical, 10).padding(.horizontal, 16)
            .background(selectedTab == tab ? Theme.accent : Color.clear)
            .cornerRadius(8)
        }
        .buttonStyle(PlainButtonStyle()).padding(.horizontal, 8)
    }

    @ViewBuilder
    private var mainContent: some View {
        switch selectedTab {
        case "receive": ReceiveView(vm: vm, selectedTransferId: $selectedTransferId)
        case "send": SendView(vm: vm)
        case "monitor": MonitorView(vm: vm)
        case "network": NetworkView(vm: vm)
        case "health": HealthView(vm: vm)
        case "settings": SettingsView()
        default: ReceiveView(vm: vm, selectedTransferId: $selectedTransferId)
        }
    }
}

// MARK: - LocalSend-style Receive View

struct ReceiveView: View {
    @ObservedObject var vm: AppViewModel
    @Binding var selectedTransferId: String?

    var body: some View {
        HStack(spacing: 0) {
            // Local Device details & status
            VStack(alignment: .leading, spacing: 24) {
                Text("Receive").font(.largeTitle.bold()).foregroundColor(.white)

                VStack(alignment: .leading, spacing: 20) {
                    HStack(spacing: 16) {
                        ZStack {
                            Circle().fill(Theme.accent.opacity(0.15)).frame(width: 56, height: 56)
                            Image(systemName: "antenna.radiowaves.left.and.right").font(.title2).foregroundColor(Theme.accent)
                        }
                        VStack(alignment: .leading, spacing: 4) {
                            Text(Host.current().localizedName ?? "macOS Node").font(.title3.bold()).foregroundColor(.white)
                            Text("Ready to receive files").font(.subheadline).foregroundColor(Theme.textSecondary)
                        }
                    }

                    Divider().background(Theme.border)

                    VStack(spacing: 12) {
                        HStack {
                            Text("Discovery status").foregroundColor(Theme.textSecondary)
                            Spacer()
                            Text("Active / Listening").foregroundColor(Theme.green).fontWeight(.bold)
                        }
                        HStack {
                            Text("Encryption").foregroundColor(Theme.textSecondary)
                            Spacer()
                            Text("TLS Secured").foregroundColor(Theme.purple).fontWeight(.bold)
                        }
                    }
                    .font(.body)
                }
                .padding(24)
                .background(Theme.card)
                .cornerRadius(16)

                Spacer()
            }
            .padding(32)
            .frame(maxWidth: .infinity, alignment: .leading)

            Divider().background(Theme.border)

            // Transfers History & Reports Panel
            TransfersView(vm: vm, selectedId: $selectedTransferId)
                .frame(width: 500)
        }
        .background(Theme.background)
    }
}

// MARK: - LocalSend-style Send View

struct SendView: View {
    @ObservedObject var vm: AppViewModel
    @State private var queuedUrls: [URL] = []
    @State private var manualIp: String = ""
    @State private var isConnectingDirectly = false
    @State private var connectionMessage = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("Send").font(.largeTitle.bold()).foregroundColor(.white)

                // Queued Files Section
                VStack(alignment: .leading, spacing: 16) {
                    HStack {
                        Text("FILES TO SEND").font(.caption).foregroundColor(Theme.textSecondary).fontWeight(.bold)
                        Spacer()
                        if !queuedUrls.isEmpty {
                            Button("Clear Queue") {
                                queuedUrls.removeAll()
                            }
                            .foregroundColor(Theme.red)
                            .buttonStyle(PlainButtonStyle())
                        }
                    }

                    if queuedUrls.isEmpty {
                        Text("No files queued. Add files using the button below.")
                            .foregroundColor(Theme.textSecondary)
                            .padding()
                            .frame(maxWidth: .infinity, alignment: .center)
                            .background(Theme.card)
                            .cornerRadius(12)
                    } else {
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(queuedUrls, id: \.self) { url in
                                HStack {
                                    Image(systemName: "doc.fill").foregroundColor(Theme.accent)
                                    Text(url.lastPathComponent).foregroundColor(.white).lineLimit(1)
                                    Spacer()
                                    Text("\(String(format: "%.1f", (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize).map { Double($0) / 1_048_576 } ?? 0.0)) MB")
                                        .foregroundColor(Theme.textSecondary)
                                        .font(.caption)
                                    
                                    Button(action: {
                                        if let idx = queuedUrls.firstIndex(of: url) {
                                            queuedUrls.remove(at: idx)
                                        }
                                    }) {
                                        Image(systemName: "xmark.circle.fill")
                                            .foregroundColor(Theme.red)
                                    }
                                    .buttonStyle(PlainButtonStyle())
                                    .padding(.leading, 8)
                                }
                                .padding(.horizontal, 12)
                                .padding(.vertical, 8)
                                .background(Color.black.opacity(0.2))
                                .cornerRadius(6)
                            }
                        }
                        .padding()
                        .background(Theme.card)
                        .cornerRadius(12)
                    }

                    Button(action: {
                        let panel = NSOpenPanel()
                        panel.allowsMultipleSelection = true
                        panel.canChooseFiles = true
                        panel.canChooseDirectories = true
                        if panel.runModal() == .OK {
                            var processedUrls: [URL] = []
                            for url in panel.urls {
                                var isDir: ObjCBool = false
                                if FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) {
                                    if isDir.boolValue {
                                        if let zippedUrl = zipDirectory(at: url) {
                                            processedUrls.append(zippedUrl)
                                        }
                                    } else {
                                        processedUrls.append(url)
                                    }
                                }
                            }
                            queuedUrls.append(contentsOf: processedUrls)
                        }
                    }) {
                        HStack {
                            Image(systemName: "plus.circle.fill")
                            Text("Queue Files & Folders")
                        }
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(Theme.accent)
                        .foregroundColor(.black)
                        .cornerRadius(12)
                    }
                    .buttonStyle(PlainButtonStyle())
                }

                Divider().background(Theme.border)

                // Manual Connection Section
                VStack(alignment: .leading, spacing: 16) {
                    Text("CONNECT TO IP").font(.caption).foregroundColor(Theme.textSecondary).fontWeight(.bold)
                    
                    HStack(spacing: 12) {
                        TextField("Enter IP Address (e.g. 192.168.1.15)", text: $manualIp)
                            .textFieldStyle(PlainTextFieldStyle())
                            .padding(12)
                            .background(Color.black.opacity(0.3))
                            .foregroundColor(.white)
                            .cornerRadius(8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 8)
                                    .stroke(Theme.border, lineWidth: 1)
                            )
                        
                        Button(action: {
                            guard !manualIp.isEmpty else { return }
                            isConnectingDirectly = true
                            connectionMessage = "Connecting and handshaking..."
                            vm.connectToIP(manualIp) { success, nodeId in
                                isConnectingDirectly = false
                                if success, let nodeId = nodeId {
                                    connectionMessage = "Connected successfully to \(nodeId)!"
                                } else {
                                    connectionMessage = "Connection failed. Please check IP and port."
                                }
                            }
                        }) {
                            HStack {
                                if isConnectingDirectly {
                                    ProgressView().scaleEffect(0.5).frame(width: 16, height: 16)
                                }
                                Text("Connect & Add")
                            }
                            .padding(.horizontal, 16)
                            .padding(.vertical, 12)
                            .background(manualIp.isEmpty || isConnectingDirectly ? Color.gray.opacity(0.3) : Theme.accent)
                            .foregroundColor(manualIp.isEmpty || isConnectingDirectly ? .gray : .black)
                            .cornerRadius(8)
                        }
                        .disabled(manualIp.isEmpty || isConnectingDirectly)
                        .buttonStyle(PlainButtonStyle())
                        
                        Button(action: {
                            guard !manualIp.isEmpty && !queuedUrls.isEmpty else { return }
                            vm.sendFilesDirectly(urls: queuedUrls, toIp: manualIp)
                            queuedUrls.removeAll()
                            manualIp = ""
                            connectionMessage = "Sending files directly..."
                        }) {
                            Text("Send Directly")
                                .padding(.horizontal, 16)
                                .padding(.vertical, 12)
                                .background((manualIp.isEmpty || queuedUrls.isEmpty) ? Color.gray.opacity(0.3) : Color.blue)
                                .foregroundColor((manualIp.isEmpty || queuedUrls.isEmpty) ? .gray : .white)
                                .cornerRadius(8)
                        }
                        .disabled(manualIp.isEmpty || queuedUrls.isEmpty)
                        .buttonStyle(PlainButtonStyle())
                    }
                    
                    if !connectionMessage.isEmpty {
                        Text(connectionMessage)
                            .font(.subheadline)
                            .foregroundColor(connectionMessage.contains("Successfully") || connectionMessage.contains("Connected") ? .green : .orange)
                            .padding(.top, 4)
                    }
                }
                .padding()
                .background(Theme.card)
                .cornerRadius(12)

                Divider().background(Theme.border)

                // Discovered Devices Section
                VStack(alignment: .leading, spacing: 16) {
                    HStack {
                        Text("NEARBY DEVICES").font(.caption).foregroundColor(Theme.textSecondary).fontWeight(.bold)
                        Spacer()
                        Button(action: { vm.toggleScan() }) {
                            Text(vm.isScanning ? "Scanning..." : "Scan")
                                .foregroundColor(Theme.accent)
                        }
                        .buttonStyle(PlainButtonStyle())
                    }

                    if vm.discoveredNodes.isEmpty {
                        Text("Searching for nearby devices...")
                            .foregroundColor(Theme.textSecondary)
                            .padding()
                            .frame(maxWidth: .infinity, alignment: .center)
                            .background(Theme.card)
                            .cornerRadius(12)
                    } else {
                        VStack(spacing: 12) {
                            ForEach(vm.discoveredNodes) { node in
                                Button(action: {
                                    if !queuedUrls.isEmpty {
                                        vm.sendFiles(urls: queuedUrls)
                                        queuedUrls.removeAll()
                                    } else {
                                        vm.selectedNode = node
                                    }
                                }) {
                                    HStack {
                                        Image(systemName: "iphone").font(.title2).foregroundColor(.white)
                                        VStack(alignment: .leading, spacing: 4) {
                                            Text(node.name).foregroundColor(.white).fontWeight(.bold)
                                            Text(node.platform).foregroundColor(Theme.textSecondary).font(.caption)
                                        }
                                        Spacer()
                                        if !queuedUrls.isEmpty {
                                            Text("Send Queued Files").foregroundColor(Theme.accent).font(.caption).fontWeight(.bold)
                                        } else {
                                            Circle().fill(Theme.green).frame(width: 8, height: 8)
                                        }
                                    }
                                    .padding()
                                    .background(Theme.card)
                                    .cornerRadius(12)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 12)
                                            .stroke(Theme.accent.opacity(vm.selectedNode?.id == node.id ? 1.0 : 0.0), lineWidth: 1.5)
                                    )
                                }
                                .buttonStyle(PlainButtonStyle())
                            }
                        }
                    }
                }
            }
            .padding(32)
        }
        .background(Theme.background)
        .onAppear {
            loadQueue()
        }
        .onChange(of: queuedUrls) { newValue in
            saveQueue(newValue)
        }
    }

    private func saveQueue(_ urls: [URL]) {
        let paths = urls.map { $0.path }
        UserDefaults.standard.set(paths, forKey: "queued_file_paths")
    }

    private func loadQueue() {
        if let paths = UserDefaults.standard.stringArray(forKey: "queued_file_paths") {
            self.queuedUrls = paths.map { URL(fileURLWithPath: $0) }
        }
    }

    private func zipDirectory(at folderUrl: URL) -> URL? {
        let fileManager = FileManager.default
        let tempDir = fileManager.temporaryDirectory
        let zipFileName = folderUrl.lastPathComponent + ".zip"
        let destinationUrl = tempDir.appendingPathComponent(zipFileName)
        
        try? fileManager.removeItem(at: destinationUrl)
        
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        process.arguments = ["-c", "-k", "--sequesterRsrc", folderUrl.path, destinationUrl.path]
        
        do {
            try process.run()
            process.waitUntilExit()
            if process.terminationStatus == 0 {
                return destinationUrl
            }
        } catch {
            print("Error zipping directory: \(error)")
        }
        return nil
    }
}

// MARK: - Transfers View

struct TransfersView: View {
    @ObservedObject var vm: AppViewModel
    @Binding var selectedId: String?
    @State private var showReport = false

    var body: some View {
        HStack(spacing: 0) {
            // History list
            VStack(alignment: .leading) {
                Text("Transfer History").font(.title2.bold()).foregroundColor(.white).padding()
                if vm.transferHistory.isEmpty {
                    Spacer()
                    Text("No transfers yet.").foregroundColor(Theme.textSecondary).frame(maxWidth: .infinity)
                    Spacer()
                } else {
                    List(vm.transferHistory) { item in
                        Button(action: {
                            selectedId = item.id
                            vm.loadTransferReport(id: item.id)
                            showReport = true
                        }) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(item.filename).foregroundColor(.white).fontWeight(.semibold)
                                HStack {
                                    if let s = item.size { Text("\(String(format: "%.1f", Double(s)/1_048_576)) MB").foregroundColor(Theme.textSecondary).font(.caption) }
                                    if let sp = item.average_speed_mbps { Text("\(String(format: "%.1f", sp)) Mbps").foregroundColor(Theme.textSecondary).font(.caption) }
                                    if let h = item.health_score { Text("Score: \(Int(h))").foregroundColor(Theme.green).font(.caption) }
                                }
                            }.padding(8)
                        }
                        .buttonStyle(PlainButtonStyle())
                        .listRowBackground(Theme.card)
                    }
                    .listStyle(.plain)
                }
            }
            .frame(width: 300)
            .background(Theme.sidebar)

            // Detail / Report
            VStack(alignment: .leading) {
                if showReport, let r = vm.selectedReport {
                    TransferReportView(report: r, vm: vm)
                } else {
                    VStack {
                        Text("Select a transfer").foregroundColor(Theme.textSecondary)
                        if vm.transferHistory.first != nil {
                            Text("or view the latest").foregroundColor(Theme.textSecondary).font(.caption)
                        }
                    }.frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }.background(Theme.background)
        }
    }
}

struct TransferReportView: View {
    let report: TransferReport
    @ObservedObject var vm: AppViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Transfer Report").font(.title.bold()).foregroundColor(.white)

                // Summary
                Group {
                    SectionHeader("Summary")
                    KeyValueRow("Transfer ID", report.transfer_summary?.transfer_id ?? "--")
                    KeyValueRow("Duration", report.transfer_summary?.duration_secs.map { String(format: "%.1fs", $0) } ?? "--")
                }

                // File
                Group {
                    SectionHeader("File")
                    KeyValueRow("Name", report.file?.name ?? "--")
                    KeyValueRow("SHA256", report.file?.sha256.map { String($0.prefix(16)) + "..." } ?? "--")
                    if let orig = report.file?.original_size {
                        KeyValueRow("Size", "\(String(format: "%.1f", Double(orig)/1_048_576)) MB")
                    }
                    if let comp = report.file?.compressed_size {
                        KeyValueRow("Compressed", "\(String(format: "%.1f", Double(comp)/1_048_576)) MB")
                    }
                    if let ratio = report.file?.compression_ratio {
                        KeyValueRow("Ratio", String(format: "%.3f", ratio))
                    }
                }

                // Speed
                Group {
                    SectionHeader("Speed")
                    if let s = report.transfer {
                        KeyValueRow("Average", s.average_speed_mbps.map { String(format: "%.1f Mbps", $0) } ?? "--")
                        KeyValueRow("Peak", s.peak_speed_mbps.map { String(format: "%.1f Mbps", $0) } ?? "--")
                        KeyValueRow("95th %ile", s.p95_speed_mbps.map { String(format: "%.1f Mbps", $0) } ?? "--")
                    }
                }

                // Health
                Group {
                    SectionHeader("Health")
                    if let h = report.health {
                        HStack {
                            Text("Score").foregroundColor(Theme.textSecondary)
                            Spacer()
                            Text(h.health_score.map { "\(Int($0))/100" } ?? "--")
                                .foregroundColor((h.health_score ?? 100) > 70 ? Theme.green : Theme.orange)
                                .font(.title).fontWeight(.bold)
                        }
                        KeyValueRow("Bottleneck", h.bottleneck ?? "--")
                        KeyValueRow("Recommendation", h.recommendation ?? "--")
                    }
                }

                // Waterfall
                if let phases = report.waterfall, !phases.isEmpty {
                    SectionHeader("Waterfall Timeline")
                    WaterfallView(phases: phases)
                }

                // Speed graph
                if let samples = report.speed_samples, !samples.isEmpty {
                    SectionHeader("Speed Over Time")
                    SpeedGraphView(samples: samples)
                }

                // Network changes
                if let changes = report.network_changes, !changes.isEmpty {
                    SectionHeader("Network Changes")
                    ForEach(changes) { c in
                        HStack {
                            Text(c.time).foregroundColor(Theme.textSecondary).font(.caption)
                            Text(c.event ?? "").foregroundColor(.white).font(.caption)
                            Spacer()
                        }.padding(4).background(Theme.card).cornerRadius(4)
                    }
                }

                // Export button
                Button("Export Session (JSON)") {
                    vm.exportSession(id: report.transfer_summary?.transfer_id ?? "unknown")
                }
                .buttonStyle(.bordered).tint(.white)
                .padding(.top)
            }.padding(24)
        }
    }
}

struct WaterfallView: View {
    let phases: [PhaseItem]
    var body: some View {
        VStack(spacing: 6) {
            let total = max(1, phases.compactMap { $0.duration_ms }.reduce(0, +))
            ForEach(phases) { p in
                let pct = max(2, ((p.duration_ms ?? 0) / total) * 100)
                let colors: [String: Color] = ["Discovery": Theme.purple, "Authentication": Theme.blue, "Compression": Theme.orange, "TLS": Theme.green, "Streaming": Theme.accent, "Hash": .gray, "Archive": .brown]
                HStack(spacing: 8) {
                    Text(p.name).foregroundColor(Theme.textSecondary).frame(width: 100, alignment: .trailing).font(.caption)
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            RoundedRectangle(cornerRadius: 4).fill(Color.gray.opacity(0.2)).frame(height: 18)
                            RoundedRectangle(cornerRadius: 4).fill(colors[p.name, default: Theme.accent]).frame(width: geo.size.width * CGFloat(pct / 100), height: 18)
                        }
                    }.frame(height: 18)
                    Text(p.duration_ms.map { String(format: "%.0fms", $0) } ?? "--").foregroundColor(Theme.textSecondary).font(.caption).frame(width: 60)
                }
            }
        }
    }
}

struct SpeedGraphView: View {
    let samples: [SpeedSample]
    var body: some View {
        GeometryReader { geo in
            let maxSpeed = samples.compactMap { $0.speed_mbps }.max() ?? 1
            let height = geo.size.height - 20
            HStack(alignment: .bottom, spacing: 2) {
                ForEach(samples) { s in
                    let pct = CGFloat((s.speed_mbps ?? 0) / maxSpeed)
                    RoundedRectangle(cornerRadius: 2).fill(Theme.accent).frame(width: 4, height: CGFloat(pct) * height)
                }
            }
        }.frame(height: 80)
    }
}

// MARK: - Monitor View

struct MonitorView: View {
    @ObservedObject var vm: AppViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("System Monitor").font(.largeTitle.bold()).foregroundColor(.white)

                // Live Metrics
                SectionHeader("Live Metrics (macOS)")
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 180))], spacing: 12) {
                    MetricCard(title: "macOS CPU", value: (vm.systemMetrics?.cpu_usage).flatMap { "\($0)%" } ?? "--")
                    MetricCard(title: "macOS Memory", value: (vm.systemMetrics?.memory_mb).flatMap { "\($0) MB" } ?? "--")
                    MetricCard(title: "TX Speed", value: (vm.systemMetrics?.network_tx_mbps).flatMap { "\($0) Mbps" } ?? "--")
                    MetricCard(title: "RX Speed", value: (vm.systemMetrics?.network_rx_mbps).flatMap { "\($0) Mbps" } ?? "--")
                }

                SectionHeader("Live Metrics (Android Agent)")
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 180))], spacing: 12) {
                    MetricCard(title: "Android CPU", value: (vm.systemMetrics?.android_cpu).flatMap { "\($0)%" } ?? "--", color: Theme.green)
                    MetricCard(title: "Android Memory", value: (vm.systemMetrics?.android_ram).flatMap { "\($0) MB" } ?? "--", color: Theme.green)
                }

                // Process-Level
                if let p = vm.systemMetrics?.processes {
                    SectionHeader("Process-Level")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 180))], spacing: 12) {
                        MetricCard(title: "Daemon CPU", value: p.rust_daemon?.cpu.map { String(format: "%.1f%%", $0) } ?? "--")
                        MetricCard(title: "Daemon RAM", value: p.rust_daemon?.ram_mb.map { String(format: "%.1f MB", $0) } ?? "--")
                        MetricCard(title: "Hash CPU", value: p.hash_thread?.cpu.map { String(format: "%.1f%%", $0) } ?? "--")
                        MetricCard(title: "Hash RAM", value: p.hash_thread?.ram_mb.map { String(format: "%.1f MB", $0) } ?? "--")
                    }
                }

                // Thermal
                if let t = vm.systemMetrics?.thermal {
                    SectionHeader("Thermal")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 180))], spacing: 12) {
                        MetricCard(title: "CPU Temp", value: t.cpu_temp_c.map { String(format: "%.1f°C", $0) } ?? "--")
                        MetricCard(title: "Fan", value: t.fan_rpm.map { String(format: "%.0f RPM", $0) } ?? "--")
                        MetricCard(title: "Battery", value: t.battery_pct.map { String(format: "%.0f%%", $0) } ?? "--")
                        MetricCard(title: "State", value: t.thermal_state ?? "--")
                    }
                }

                // Buffer Analysis
                if let b = vm.bufferMetrics {
                    SectionHeader("Buffer Pipeline")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 180))], spacing: 12) {
                        MetricCard(title: "Read Buffer", value: b.read_buffer_kb.map { "\($0) KB" } ?? "--")
                        MetricCard(title: "Write Buffer", value: b.write_buffer_kb.map { "\($0) KB" } ?? "--")
                        MetricCard(title: "Avg Queue", value: b.average_queue_depth.map { String(format: "%.1f", $0) } ?? "--")
                        MetricCard(title: "Backpressure", value: b.backpressure_events.map { "\($0) events" } ?? "--")
                    }
                }
            }.padding(32)
        }.background(Theme.background)
    }
}

// MARK: - Network View

struct NetworkView: View {
    @ObservedObject var vm: AppViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("Network").font(.largeTitle.bold()).foregroundColor(.white)

                // Protocol Statistics
                if let ps = vm.protocolStats {
                    SectionHeader("Protocol Statistics")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 130))], spacing: 12) {
                        ProtoStatCard(label: "Discovery", value: ps.discovery_packets ?? 0)
                        ProtoStatCard(label: "Auth", value: ps.auth_requests ?? 0)
                        ProtoStatCard(label: "Transfers", value: ps.transfer_requests ?? 0)
                        ProtoStatCard(label: "Resume", value: ps.resume_requests ?? 0)
                        ProtoStatCard(label: "Completed", value: ps.completed_transfers ?? 0)
                        ProtoStatCard(label: "Failed", value: ps.failed_transfers ?? 0)
                        ProtoStatCard(label: "TLS", value: ps.tls_handshakes ?? 0)
                        ProtoStatCard(label: "Range", value: ps.range_requests ?? 0)
                    }
                }

                // Network Path
                SectionHeader("Network Path")
                if vm.networkPath.isEmpty {
                    Text("No network data yet.").foregroundColor(Theme.textSecondary)
                } else {
                    ForEach(vm.networkPath.suffix(20)) { item in
                        HStack {
                            Text(item.time ?? "").foregroundColor(Theme.textSecondary).font(.caption).frame(width: 70, alignment: .leading)
                            Text(item.interface ?? "--").foregroundColor(.white).font(.caption).frame(width: 80)
                            Text(item.ip ?? "--").foregroundColor(Theme.textSecondary).font(.caption)
                            Spacer()
                            if let rssi = item.rssi { Text("\(String(format: "%.0f", rssi)) dBm").foregroundColor(Theme.textSecondary).font(.caption) }
                            Text(item.event ?? "").foregroundColor(item.event == "Disconnected" ? Theme.red : Theme.green).font(.caption).frame(width: 90)
                        }.padding(8).background(Theme.card).cornerRadius(6)
                    }
                }
            }.padding(32)
        }.background(Theme.background)
    }
}

// MARK: - Health View

struct HealthView: View {
    @ObservedObject var vm: AppViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("System Health").font(.largeTitle.bold()).foregroundColor(.white)

                // Overall Health Score
                if let h = vm.healthScore {
                    HStack(spacing: 24) {
                        ZStack {
                            Circle().stroke(Theme.border, lineWidth: 8).frame(width: 100, height: 100)
                            Circle().trim(from: 0, to: CGFloat((h.overall ?? 0) / 100)).stroke(Theme.accent, lineWidth: 8).frame(width: 100, height: 100).rotationEffect(.degrees(-90))
                            Text("\(Int(h.overall ?? 0))").foregroundColor(.white).font(.title).fontWeight(.bold)
                        }
                        Text("Overall Health").foregroundColor(Theme.textSecondary)
                        Spacer()
                    }

                    VStack(spacing: 8) {
                        HealthBar(label: "CPU", value: h.cpu ?? 0)
                        HealthBar(label: "Network", value: h.network ?? 0)
                        HealthBar(label: "Disk", value: h.disk ?? 0)
                        HealthBar(label: "Integrity", value: h.integrity ?? 100)
                        HealthBar(label: "Recovery", value: h.recovery ?? 100)
                    }.padding().background(Theme.card).cornerRadius(12)
                }

                // Storage Forecast
                if let s = vm.storageInfo {
                    SectionHeader("Storage Forecast")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 150))], spacing: 12) {
                        MetricCard(title: "Total", value: s.total_gb.map { String(format: "%.1f GB", $0) } ?? "--")
                        MetricCard(title: "Free", value: s.free_gb.map { String(format: "%.1f GB", $0) } ?? "--")
                        MetricCard(title: "Remaining", value: s.remaining_gb.map { String(format: "%.1f GB", $0) } ?? "--")
                        MetricCard(title: "Status", value: (s.enough_space ?? false) ? "Enough Space" : "Low Space", color: (s.enough_space ?? false) ? Theme.green : Theme.red)
                    }
                }

                // Compression Analytics
                if !vm.compressionAnalytics.isEmpty {
                    SectionHeader("Compression Analytics")
                    ForEach(vm.compressionAnalytics) { item in
                        HStack {
                            Text(item.filename).foregroundColor(.white).font(.caption).frame(width: 150, alignment: .leading)
                            if let orig = item.original_size, let comp = item.compressed_size {
                                Text("\(String(format: "%.1f", Double(orig)/1_048_576)) → \(String(format: "%.1f", Double(comp)/1_048_576)) MB").foregroundColor(Theme.textSecondary).font(.caption)
                            }
                            if let ratio = item.compression_ratio {
                                Text("Ratio: \(String(format: "%.2f", ratio))").foregroundColor(Theme.green).font(.caption)
                            }
                            if let saved = item.bandwidth_saved {
                                Text("Saved: \(String(format: "%.1f", Double(saved)/1_048_576)) MB").foregroundColor(Theme.orange).font(.caption)
                            }
                            Spacer()
                        }.padding(8).background(Theme.card).cornerRadius(6)
                    }
                }

                // Bottleneck Detection
                if let b = vm.bottleneck {
                    SectionHeader("Bottleneck Detection")
                    HStack {
                        Text("Current: ").foregroundColor(Theme.textSecondary)
                        Text(b.bottleneck ?? "Idle").foregroundColor(.white).fontWeight(.bold)
                        Spacer()
                        Text(b.recommendation ?? "").foregroundColor(Theme.textSecondary).font(.caption)
                    }.padding().background(Theme.card).cornerRadius(12)
                }

                // Export & Replay
                SectionHeader("Session Tools")
                HStack(spacing: 16) {
                    if let last = vm.transferHistory.first {
                        Button("Export Latest Session") { vm.exportSession(id: last.id) }
                            .buttonStyle(.bordered).tint(.white)
                    }
                    Button("Export All History") {
                        let desktop = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask).first!
                        let url = desktop.appendingPathComponent("pdos_history.json")
                        if let data = try? JSONEncoder().encode(vm.transferHistory) {
                            try? data.write(to: url)
                            NSWorkspace.shared.activateFileViewerSelecting([url as URL])
                        }
                    }.buttonStyle(.bordered).tint(Theme.orange)

                    Button("Protocol Replay") {
                        NSWorkspace.shared.open(URL(string: "http://127.0.0.1:8080")!)
                    }.buttonStyle(.bordered).tint(Theme.blue)
                }
            }.padding(32)
        }.background(Theme.background)
    }
}

// MARK: - Settings View

struct SettingsView: View {
    @State private var autostart = false
    @State private var autoFinish = true
    @State private var saveHistory = true
    @State private var pin: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Settings").font(.largeTitle.bold()).foregroundColor(.white)
            Group {
                Toggle("Autostart after login", isOn: $autostart).toggleStyle(.switch).tint(.white)
                Toggle("Auto-finish transfers", isOn: $autoFinish).toggleStyle(.switch).tint(.white)
                Toggle("Save transfer history", isOn: $saveHistory).toggleStyle(.switch).tint(.white)
            }.foregroundColor(.white).padding().background(Theme.card).cornerRadius(12)

            VStack(alignment: .leading, spacing: 12) {
                Text("Pairing").foregroundColor(.white).fontWeight(.bold)
                Button("Generate Pairing PIN") {
                    guard let url = URL(string: "http://127.0.0.1:8080/api/generate-pin") else { return }
                    var req = URLRequest(url: url); req.httpMethod = "POST"
                    URLSession.shared.dataTask(with: req) { data, _, _ in
                        guard let data = data, let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let p = json["pin"] as? String else { return }
                        DispatchQueue.main.async { pin = p }
                    }.resume()
                }.buttonStyle(.bordered).tint(.white)
                if let p = pin {
                    Text(p).font(.system(size: 28, design: .monospaced)).foregroundColor(.white).padding().background(Color.black.opacity(0.3)).cornerRadius(8)
                }
            }.padding().background(Theme.card).cornerRadius(12)
            Spacer()
        }.padding(32).background(Theme.background)
    }
}

// MARK: - Reusable Components

struct SectionHeader: View {
    let title: String
    init(_ title: String) { self.title = title }
    var body: some View {
        Text(title).font(.headline).foregroundColor(Theme.textSecondary).padding(.top, 8)
    }
}

struct KeyValueRow: View {
    let key: String; let value: String
    init(_ key: String, _ value: String) { self.key = key; self.value = value }
    var body: some View {
        HStack {
            Text(key).foregroundColor(Theme.textSecondary).font(.caption).frame(width: 140, alignment: .leading)
            Text(value).foregroundColor(.white).font(.caption)
            Spacer()
        }.padding(4)
    }
}

struct MetricCard: View {
    let title: String; let value: String; var color: Color = .white
    var body: some View {
        VStack(spacing: 8) {
            Text(title).font(.caption).foregroundColor(Theme.textSecondary)
            Text(value).font(.title2).fontWeight(.bold).foregroundColor(color)
        }.padding().frame(maxWidth: .infinity).background(Theme.card).cornerRadius(12)
    }
}

struct ProtoStatCard: View {
    let label: String; let value: Int
    var body: some View {
        VStack(spacing: 4) {
            Text("\(value)").font(.title2).fontWeight(.bold).foregroundColor(.white)
            Text(label).font(.caption).foregroundColor(Theme.textSecondary)
        }.padding().frame(maxWidth: .infinity).background(Theme.card).cornerRadius(12)
    }
}

struct HealthBar: View {
    let label: String; let value: Double
    var body: some View {
        HStack(spacing: 12) {
            Text(label).foregroundColor(.white).font(.caption).frame(width: 70, alignment: .leading)
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 4).fill(Color.gray.opacity(0.2)).frame(height: 8)
                    RoundedRectangle(cornerRadius: 4).fill(Theme.accent).frame(width: geo.size.width * CGFloat(value / 100), height: 8)
                }
            }.frame(height: 8)
            Text("\(Int(value))").foregroundColor(Theme.textSecondary).font(.caption).frame(width: 30)
        }
    }
}
