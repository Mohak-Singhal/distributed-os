import Foundation

struct KnownRelay: Codable, Identifiable, Equatable {
    let id: String
    let host: String
    let port: Int
    var lastConnected: Date
    var name: String

    var url: String { "\(host):\(port)" }
}

struct KnownDevice: Codable, Identifiable {
    let id: String
    let name: String
    let platform: String
}

struct DevicePersistence {
    private static let knownRelaysKey = "KnownRelays"
    private static let lastRelayKey = "LastConnectedRelay"
    private static let autoConnectKey = "AutoConnectToRelay"

    static var knownRelays: [KnownRelay] {
        get {
            guard let data = UserDefaults.standard.data(forKey: knownRelaysKey),
                  let relays = try? JSONDecoder().decode([KnownRelay].self, from: data) else {
                return []
            }
            return relays.sorted { $0.lastConnected > $1.lastConnected }
        }
        set {
            guard let data = try? JSONEncoder().encode(newValue) else { return }
            UserDefaults.standard.set(data, forKey: knownRelaysKey)
        }
    }

    static var lastRelay: KnownRelay? {
        get {
            guard let data = UserDefaults.standard.data(forKey: lastRelayKey),
                  let relay = try? JSONDecoder().decode(KnownRelay.self, from: data) else {
                return nil
            }
            return relay
        }
        set {
            guard let data = try? JSONEncoder().encode(newValue) else { return }
            UserDefaults.standard.set(data, forKey: lastRelayKey)
        }
    }

    static var autoConnectEnabled: Bool {
        get { UserDefaults.standard.bool(forKey: autoConnectKey) }
        set { UserDefaults.standard.set(newValue, forKey: autoConnectKey) }
    }

    static func addRelay(host: String, port: Int, name: String? = nil) {
        let id = "\(host):\(port)"
        var relays = knownRelays
        relays.removeAll { $0.id == id }
        let relay = KnownRelay(
            id: id,
            host: host,
            port: port,
            lastConnected: Date(),
            name: name ?? host
        )
        relays.insert(relay, at: 0)
        knownRelays = relays
        lastRelay = relay
    }

    static func removeRelay(id: String) {
        var relays = knownRelays
        relays.removeAll { $0.id == id }
        knownRelays = relays
        if lastRelay?.id == id {
            lastRelay = nil
        }
    }

    static func updateLastConnected(for id: String) {
        var relays = knownRelays
        guard let idx = relays.firstIndex(where: { $0.id == id }) else { return }
        relays[idx].lastConnected = Date()
        knownRelays = relays
        lastRelay = relays[idx]
    }

    // ── Trusted Devices ──

    private static let trustedDevicesKey = "TrustedDevices"

    static var trustedDevices: [TrustedDevice] {
        get {
            guard let data = UserDefaults.standard.data(forKey: trustedDevicesKey),
                  let devices = try? JSONDecoder().decode([TrustedDevice].self, from: data) else {
                return []
            }
            return devices.sorted { $0.lastSeen > $1.lastSeen }
        }
        set {
            guard let data = try? JSONEncoder().encode(newValue) else { return }
            UserDefaults.standard.set(data, forKey: trustedDevicesKey)
        }
    }

    static func addTrustedDevice(id: String, name: String, fingerprint: String, autoAccept: Bool = true) {
        var devices = trustedDevices
        devices.removeAll { $0.id == id }
        let device = TrustedDevice(
            id: id,
            name: name,
            fingerprint: fingerprint,
            lastSeen: Date(),
            autoAccept: autoAccept,
            allowedDirectories: ["~/Downloads/PDOS"]
        )
        devices.insert(device, at: 0)
        trustedDevices = devices
    }

    static func removeTrustedDevice(id: String) {
        var devices = trustedDevices
        devices.removeAll { $0.id == id }
        trustedDevices = devices
    }

    static func updateTrustedDevice(id: String, autoAccept: Bool? = nil, allowedDirectories: [String]? = nil) {
        var devices = trustedDevices
        guard let idx = devices.firstIndex(where: { $0.id == id }) else { return }
        if let aa = autoAccept { devices[idx].autoAccept = aa }
        if let dirs = allowedDirectories { devices[idx].allowedDirectories = dirs }
        devices[idx].lastSeen = Date()
        trustedDevices = devices
    }

    static func isDeviceTrusted(id: String) -> Bool {
        trustedDevices.contains { $0.id == id }
    }

    // ── Known Devices for Drop Target ──

    static var knownDevices: [KnownDevice] {
        return trustedDevices.map { device in
            KnownDevice(
                id: device.id,
                name: device.name,
                platform: "android"
            )
        }
    }
}
