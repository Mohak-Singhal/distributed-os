import Foundation

/// Represents a device that has been successfully paired via QR code.
struct PairedDevice: Codable, Identifiable {
    let id: String
    let name: String
    let platform: String
    let relayURL: String?
    let pairedAt: Date

    enum CodingKeys: String, CodingKey {
        case id = "node_id"
        case name = "node_name"
        case platform
        case relayURL = "relay_url"
        case pairedAt = "paired_at"
    }
}
