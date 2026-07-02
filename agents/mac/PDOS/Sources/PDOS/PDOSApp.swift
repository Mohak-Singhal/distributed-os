import SwiftUI

class AppDelegate: NSObject, NSApplicationDelegate {
    var daemonProcess: Process?

    func applicationDidFinishLaunching(_ notification: Notification) {
        startDaemon()
    }

    func applicationWillTerminate(_ notification: Notification) {
        daemonProcess?.terminate()
    }

    private func startDaemon() {
        if let daemonPath = Bundle.main.path(forResource: "dos", ofType: nil) {
            let task = Process()
            task.launchPath = daemonPath
            task.arguments = ["dashboard"]
            
            // Send output to /dev/null so it doesn't clutter Console.app or block
            task.standardOutput = FileHandle.nullDevice
            task.standardError = FileHandle.nullDevice
            
            do {
                try task.run()
                daemonProcess = task
                print("PDOS Daemon started from bundle.")
            } catch {
                print("Failed to start bundled daemon: \(error)")
            }
        } else {
            print("WARNING: Bundled 'dos' daemon not found in Resources!")
        }
    }
}

@main
struct PDOSApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 900, minHeight: 650)
        }
        .windowStyle(DefaultWindowStyle())
    }
}
