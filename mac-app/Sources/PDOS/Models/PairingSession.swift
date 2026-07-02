import Foundation

/// An active pairing session displayed as a QR code.
struct PairingSession: Equatable {
    let token: String
    let port: UInt16
    let createdAt: Date
    let nodeID: String
    let nodeName: String
    let ipAddresses: [String]

    /// The URL-encoded payload embedded in the QR code.
    var qrPayload: String {
        let ip = ipAddresses.first ?? "127.0.0.1"
        return "pdos://\(ip):\(port)/pair?token=\(token)"
    }

    var isValid: Bool {
        Date().timeIntervalSince(createdAt) < 300
    }
}
