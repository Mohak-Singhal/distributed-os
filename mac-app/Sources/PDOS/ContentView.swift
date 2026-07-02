import SwiftUI

enum AppTab: String, CaseIterable {
    case transfer = "Transfer"
    case devices = "Devices"
    case settings = "Settings"

    var icon: String {
        switch self {
        case .transfer: return "arrow.up.doc"
        case .devices: return "antenna.radiowaves.left.and.right"
        case .settings: return "gearshape"
        }
    }
}

struct ContentView: View {
    @EnvironmentObject var backend: BackendService
    @EnvironmentObject var connectionManager: ConnectionManager
    @EnvironmentObject var hotspotDetector: HotspotDetector

    @State private var selectedTab: AppTab = .transfer

    var body: some View {
        NavigationSplitView {
            sidebar
                .background(EffectView(material: .sidebar, blendingMode: .behindWindow))
                .frame(minWidth: 180)
        } detail: {
            ZStack {
                EffectView(material: .contentBackground, blendingMode: .behindWindow)
                    .ignoresSafeArea()

                switch selectedTab {
                case .transfer: TransferView()
                case .devices: DevicesView()
                case .settings: SettingsView()
                }
            }
        }
        .frame(minWidth: 800, minHeight: 500)
        .task {
            if DevicePersistence.autoConnectEnabled && !backend.relayRunning {
                backend.startRelay()
            }
        }
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            header
                .padding(.horizontal, 12)
                .padding(.vertical, 16)

            List(AppTab.allCases, id: \.self, selection: $selectedTab) { tab in
                Label(tab.rawValue, systemImage: tab.icon)
                    .font(.subheadline)
                    .padding(8)
                    .background(
                        ZStack {
                            if selectedTab == tab {
                                RoundedRectangle(cornerRadius: 10)
                                    .fill(Color.brandCyan.opacity(0.15))
                            }
                        }
                    )
                    .contentShape(Rectangle())
                    .onTapGesture {
                        withAnimation(.spring(duration: 0.35, bounce: 0.18)) {
                            selectedTab = tab
                        }
                    }
            }
            .navigationTitle("")
            .scrollContentBackground(.hidden)

            Spacer()

            bottomSection
                .padding(12)
        }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Image(systemName: "square.stack.3d.up.fill")
                .font(.system(size: 16))
                .foregroundColor(.brandCyan)

            Text("PDOS")
                .font(.system(size: 15, weight: .semibold))
        }
    }

    private var bottomSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Divider()

            HStack(spacing: 6) {
                Circle()
                    .fill(connectionManager.isConnected ? Color.green : Color.gray)
                    .frame(width: 6, height: 6)

                Text(connectionManager.isConnected ? "Connected" : "Disconnected")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
        }
    }
}
