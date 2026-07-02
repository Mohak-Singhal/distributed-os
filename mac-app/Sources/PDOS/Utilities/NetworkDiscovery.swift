import Foundation
import Network

/// Discovers local network interface IPv4 addresses.
enum NetworkDiscovery {
    /// Returns non-loopback IPv4 addresses for en* and eth* interfaces.
    static func getLocalIPAddresses() -> [String] {
        var addresses: [String] = []
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&ifaddr) == 0, let firstAddr = ifaddr else { return [] }

        var ptr = firstAddr
        while true {
            let flags = Int32(ptr.pointee.ifa_flags)
            let addr = ptr.pointee.ifa_addr.pointee
            if addr.sa_family == UInt8(AF_INET) {
                let name = String(cString: ptr.pointee.ifa_name)
                if name.hasPrefix("en") || name.hasPrefix("eth") {
                    var hostname = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                    getnameinfo(
                        ptr.pointee.ifa_addr,
                        socklen_t(addr.sa_len),
                        &hostname, socklen_t(hostname.count),
                        nil, 0,
                        NI_NUMERICHOST
                    )
                    let ip = String(cString: hostname)
                    if !ip.isEmpty && ip != "127.0.0.1" {
                        addresses.append(ip)
                    }
                }
            }
            guard let next = ptr.pointee.ifa_next else { break }
            ptr = next
        }
        freeifaddrs(ifaddr)
        return addresses
    }
}
