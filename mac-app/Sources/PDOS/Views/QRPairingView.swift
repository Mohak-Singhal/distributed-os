import SwiftUI

/// Top-level container that renders the correct state view for QR pairing.
///
/// State machine:
/// - `isActive && hasSession` → `PairingActiveView`
/// - `peerPaired`             → `PairedDeviceView`
/// - otherwise                → `PairingInactiveView`
struct QRPairingView: View {
    @StateObject private var pairingService = QRPairingService.shared
    @EnvironmentObject var connectionManager: ConnectionManager
    @EnvironmentObject var backend: BackendService

    @State private var qrImage: NSImage?

    var body: some View {
        Group {
            if pairingService.isActive, let session = pairingService.session {
                PairingActiveView(
                    pairingService: pairingService,
                    session: session,
                    qrImage: qrImage ?? pairingService.generateQRImage(),
                    onRegenerate: regenerate,
                    onStop: { pairingService.stop() }
                )
            } else if pairingService.peerPaired, let device = pairingService.pairedDevice {
                PairedDeviceView(
                    device: device,
                    onConnect: { host, port in
                        connectionManager.connectToRelay(host: host, port: port, backend: backend)
                    }
                )
            } else {
                PairingInactiveView(onStart: start)
            }
        }
        .onAppear {
            qrImage = pairingService.generateQRImage()
            if !pairingService.isActive && !pairingService.peerPaired {
                pairingService.start()
            }
        }
        .onDisappear {
            pairingService.stop()
        }
        .onChange(of: pairingService.session) { _, newSession in
            if newSession != nil {
                qrImage = pairingService.generateQRImage()
            }
        }
    }

    private func start() {
        pairingService.start()
    }

    private func regenerate() {
        pairingService.stop()
        pairingService.start()
    }
}
