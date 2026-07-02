import SwiftUI

@main
struct PDOSApp: App {
    @StateObject private var backend = BackendService.shared
    @StateObject private var connectionManager = ConnectionManager.shared
    @StateObject private var hotspotDetector = HotspotDetector.shared
    @AppStorage("showInMenuBar") private var showInMenuBar = false
    @AppStorage("autoConnect") private var autoConnect = true

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(backend)
                .environmentObject(connectionManager)
                .environmentObject(hotspotDetector)
                .background(WindowAccessor())
                .onDisappear {
                    connectionManager.stop(backend: backend)
                    hotspotDetector.stopMonitoring()
                }
                .onAppear {
                    backend.requestNotificationPermission()
                    hotspotDetector.startMonitoring()
                    connectionManager.setupHotspotAutoConnect(detector: hotspotDetector, backend: backend)
                    if autoConnect {
                        connectionManager.startRelay(backend: backend)
                    }
                    ADBScanner.shared.startScanning()
                }
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .commands {
            CommandMenu("PDOS") {
                Button("Start Backend") { connectionManager.startRelay(backend: backend) }
                    .keyboardShortcut("r", modifiers: .command)
                Button("Stop Backend") { connectionManager.stop(backend: backend) }
                    .keyboardShortcut(".", modifiers: .command)
                Divider()
                Button("Refresh All") {
                    Task { await backend.refreshAll() }
                }
                .keyboardShortcut("R", modifiers: [.command, .shift])
            }
        }

        Settings {
            SettingsView()
                .environmentObject(backend)
                .environmentObject(hotspotDetector)
        }

        MenuBarExtra("PDOS", systemImage: "square.stack.3d.up.fill", isInserted: $showInMenuBar) {
            MenuBarView()
                .environmentObject(backend)
                .environmentObject(connectionManager)
        }
        .menuBarExtraStyle(.window)
    }
}
