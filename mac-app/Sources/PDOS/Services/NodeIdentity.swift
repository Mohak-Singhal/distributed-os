import Foundation

/// Manages persistent node identity for this device.
enum NodeIdentity {
    private static let nodeIDKey = "pdos_node_id"

    /// Returns the stable node ID, creating one if none exists.
    static func getNodeID() -> String {
        let defaults = UserDefaults.standard
        if let existing = defaults.string(forKey: nodeIDKey) {
            return existing
        }
        let uuid = UUID().uuidString
        defaults.set(uuid, forKey: nodeIDKey)
        return uuid
    }

    /// The human-readable name of this device.
    static var nodeName: String {
        Host.current().localizedName ?? "Mac"
    }
}
