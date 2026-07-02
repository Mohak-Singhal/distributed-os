import Foundation
import AppKit
import Network

/// Orchestrates the QR-based device pairing flow.
///
/// Composition:
/// - `PairingToken` — cryptographic one-time token
/// - `PairingServer` — lightweight HTTP server
/// - `NetworkDiscovery` — local IP detection
/// - `NodeIdentity` — persistent node ID
/// - `QRCodeGenerator` — Core Image QR generation
@MainActor
class QRPairingService: ObservableObject {
    static let shared = QRPairingService()

    @Published var isActive = false
    @Published var session: PairingSession?
    @Published var pairedDevice: PairedDevice?
    @Published var errorMessage: String?
    @Published var peerPaired = false

    private var server: PairingServer?
    private var currentToken: PairingToken?

    private init() {}

    func start() {
        stop()

        let ips = NetworkDiscovery.getLocalIPAddresses()
        guard !ips.isEmpty else {
            errorMessage = "No network interfaces available"
            return
        }

        let token = PairingToken.generate()
        currentToken = token
        server = PairingServer()

        let nodeID = NodeIdentity.getNodeID()
        let nodeName = NodeIdentity.nodeName

        Task {
            let handler: @Sendable (String) -> (status: Int, body: AnyEncodable) = { request in
                let lines = request.components(separatedBy: .newlines)
                guard let first = lines.first, first.hasPrefix("GET ") else {
                    return (400, AnyEncodable(["error": "Missing token"]))
                }
                let parts = first.components(separatedBy: " ")
                guard parts.count >= 2,
                      let url = URLComponents(string: parts[1]),
                      let extractedToken = url.queryItems?.first(where: { $0.name == "token" })?.value else {
                    return (400, AnyEncodable(["error": "Missing token"]))
                }

                guard token.matches(extractedToken), token.isValid else {
                    return (401, AnyEncodable(["error": "Invalid or expired token"]))
                }

                let relayURL = "ws://\(ips.first ?? "127.0.0.1"):7890"
                let device = PairedDevice(
                    id: nodeID,
                    name: nodeName,
                    platform: "mac",
                    relayURL: relayURL,
                    pairedAt: Date()
                )

                Task { @MainActor in
                    QRPairingService.shared.pairedDevice = device
                    QRPairingService.shared.peerPaired = true
                }

                return (200, AnyEncodable(device))
            }

            do {
                let port = try await self.server?.start(onRequest: handler) ?? 0
                await MainActor.run {
                    self.session = PairingSession(
                        token: token.value,
                        port: port,
                        createdAt: token.createdAt,
                        nodeID: nodeID,
                        nodeName: nodeName,
                        ipAddresses: ips
                    )
                    self.isActive = true
                    self.errorMessage = nil
                }
            } catch {
                await MainActor.run {
                    self.errorMessage = error.localizedDescription
                    self.isActive = false
                }
            }
        }
    }

    func stop() {
        Task { await server?.stop() }
        server = nil
        currentToken = nil
        isActive = false
        session = nil
        peerPaired = false
        pairedDevice = nil
    }

    func generateQRImage() -> NSImage? {
        guard let payload = session?.qrPayload else { return nil }
        return QRCodeGenerator.generate(from: payload)
    }


}
