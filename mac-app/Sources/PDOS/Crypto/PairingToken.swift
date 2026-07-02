import Foundation
import Security

/// Generates and validates cryptographically secure one-time pairing tokens.
struct PairingToken {
    let value: String
    let createdAt: Date

    /// 32 random bytes = 64 hex chars.
    private static let tokenByteCount = 32

    /// Tokens expire after 5 minutes.
    var isValid: Bool {
        Date().timeIntervalSince(createdAt) < 300
    }

    /// Generate a new random token using `SecRandomCopyBytes`.
    static func generate() -> Self {
        var bytes = [UInt8](repeating: 0, count: tokenByteCount)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        return Self(value: Data(bytes).hexEncodedString(), createdAt: Date())
    }

    func matches(_ other: String) -> Bool {
        value == other
    }
}

private extension Data {
    func hexEncodedString() -> String {
        map { String(format: "%02hhx", $0) }.joined()
    }
}
