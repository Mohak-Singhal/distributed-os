import Foundation

// MARK: - Network Node Model

struct PDOSNode: Identifiable, Hashable {
    let id: String
    let nodeId: String
    let name: String
    let platform: String
    let status: String
    let capabilities: [String]
    let lastSeen: Date?
    let isKnown: Bool

    var displayName: String {
        name.isEmpty ? nodeId.prefix(8).description : name
    }

    var shortId: String { String(nodeId.prefix(12)) + "..." }

    var platformIcon: String {
        PlatformMapper.icon(for: platform)
    }
}

// MARK: - Discovered Device Model

struct DiscoveredDevice: Identifiable {
    let id: String
    let name: String
    let platform: String
    let type: DeviceType
    let isOnline: Bool

    enum DeviceType {
        case usb
        case network
    }

    var platformLabel: String {
        PlatformMapper.label(for: platform)
    }
}

// MARK: - Device Action Model

struct NodeAction: Identifiable {
    let id: String
    let label: String
    let icon: String
}

// MARK: - Capability Info

struct CapabilityInfo: Identifiable {
    let id: String
    let icon: String
    let label: String

    static let all: [String: CapabilityInfo] = [
        "clipboard": CapabilityInfo(id: "clipboard", icon: "doc.on.clipboard", label: "Clipboard"),
        "terminal": CapabilityInfo(id: "terminal", icon: "terminal", label: "Terminal"),
        "notifications": CapabilityInfo(id: "notifications", icon: "bell", label: "Notify"),
        "file": CapabilityInfo(id: "file", icon: "doc", label: "Files"),
        "ping": CapabilityInfo(id: "ping", icon: "antenna.radiowaves.left.and.right", label: "Ping"),
    ]

    static func icon(for capability: String) -> String {
        all[capability]?.icon ?? "questionmark"
    }

    static func label(for capability: String) -> String {
        all[capability]?.label ?? capability.capitalized
    }
}

// MARK: - Platform Mapper

struct PlatformMapper {
    private let androidBrands = ["Pixel", "Samsung", "Galaxy", "OnePlus", "Xiaomi", "Oppo", "Vivo", "Nothing", "Motorola", "Nokia", "Sony"]

    // MARK: - Platform Icon (SF Symbol)

    static func icon(for platform: String) -> String {
        let normalized = platform.lowercased()
        switch normalized {
        case "macos", "mac": return "laptopcomputer"
        case "android": return "iphone.gen2"
        case "linux": return "desktopcomputer"
        case "windows": return "pc"
        default: return "server.rack"
        }
    }

    // MARK: - Platform Label

    static func label(for platform: String) -> String {
        let normalized = platform.lowercased()
        switch normalized {
        case "macos", "mac": return "macOS"
        case "android": return "Android"
        case "linux": return "Linux"
        case "windows": return "Windows"
        default: return platform.capitalized
        }
    }

    // MARK: - Device Emoji

    static func deviceEmoji(for name: String, platform: String) -> String {
        let normalized = platform.lowercased()
        switch normalized {
        case "macos", "mac":
            if name.localizedCaseInsensitiveContains("Mac") { return "\u{1F5A5}\u{FE0F}" }
            return "\u{1F5A5}\u{FE0F}"
        case "android":
            let brand = ["Pixel", "Samsung", "Galaxy", "OnePlus", "Xiaomi", "Oppo", "Vivo", "Nothing", "Motorola", "Nokia", "Sony"].first { name.localizedCaseInsensitiveContains($0) }
            if brand != nil { return "\u{1F4F1}" }
            return "\u{1F4F1}"
        case "linux": return "\u{1F5A5}\u{FE0F}"
        case "windows": return "\u{1F5A5}\u{FE0F}"
        default: return "\u{1F5A5}\u{FE0F}"
        }
    }
}
