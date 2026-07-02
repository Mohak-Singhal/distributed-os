import SwiftUI

/// Device type avatar with platform icon and friendly name display.
/// AirDrop-style circular icon with subtle glow for online devices.
struct DeviceAvatar: View {
    let platform: String
    let name: String
    let isOnline: Bool
    var size: CGFloat = 56

    @State private var isGlowing = false

    var body: some View {
        ZStack {
            // Background circle with gradient
            Circle()
                .fill(
                    LinearGradient(
                        colors: [
                            backgroundColor.opacity(0.3),
                            backgroundColor.opacity(0.1)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
                .frame(width: size, height: size)

            // Border ring
            Circle()
                .stroke(
                    isOnline ? Color.brandCyan.opacity(0.4) : Color.gray.opacity(0.2),
                    lineWidth: 1.5
                )
                .frame(width: size, height: size)

            // Platform icon
            Image(systemName: platformIcon)
                .font(.system(size: size * 0.4, weight: .medium))
                .foregroundColor(iconColor)
        }
        .shadow(
            color: isOnline ? Color.brandCyan.opacity(0.15) : Color.clear,
            radius: isOnline ? 8 : 0
        )
        .onAppear {
            if isOnline {
                withAnimation(.easeInOut(duration: 2).repeatForever(autoreverses: true)) {
                    isGlowing = true
                }
            }
        }
    }

    private var platformIcon: String {
        switch platform.lowercased() {
        case "macos", "mac":
            return "laptopcomputer"
        case "android":
            return "iphone.gen2"
        case "linux":
            return "desktopcomputer"
        case "windows":
            return "pc"
        default:
            return "questionmark.circle"
        }
    }

    private var iconColor: Color {
        switch platform.lowercased() {
        case "macos", "mac":
            return .white
        case "android":
            return Color.brandCyan
        case "linux":
            return Color.brandEmerald
        case "windows":
            return Color.brandIndigo
        default:
            return .secondary
        }
    }

    private var backgroundColor: Color {
        switch platform.lowercased() {
        case "macos", "mac":
            return .gray
        case "android":
            return Color.brandCyan
        case "linux":
            return Color.brandEmerald
        case "windows":
            return Color.brandIndigo
        default:
            return .gray
        }
    }
}

/// Compact version for inline device references
struct DeviceBadge: View {
    let platform: String
    let size: CGFloat = 20

    var body: some View {
        Image(systemName: platformIcon)
            .font(.system(size: size * 0.6, weight: .medium))
            .foregroundColor(.secondary)
            .frame(width: size, height: size)
    }

    private var platformIcon: String {
        switch platform.lowercased() {
        case "macos", "mac": return "laptopcomputer"
        case "android": return "iphone.gen2"
        case "linux": return "desktopcomputer"
        case "windows": return "pc"
        default: return "questionmark.circle"
        }
    }
}

#Preview {
    HStack(spacing: 20) {
        DeviceAvatar(platform: "mac", name: "MacBook Air", isOnline: true)
        DeviceAvatar(platform: "android", name: "Pixel 8 Pro", isOnline: true)
        DeviceAvatar(platform: "android", name: "Samsung S23", isOnline: false)
        DeviceAvatar(platform: "linux", name: "Ubuntu Server", isOnline: true)
    }
    .padding()
    .background(Color.black)
}
