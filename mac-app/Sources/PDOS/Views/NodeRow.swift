import SwiftUI

struct NodeRow: View {
    let node: PDOSNode
    let onAction: (String, String) -> Void
    let onConnect: (String) -> Void
    let onForget: (String) -> Void

    @Environment(\.colorScheme) var colorScheme
    @State private var isLoadingAction: String? = nil
    @State private var isConnecting = false
    @State private var connectionError: String? = nil
    @State private var isHovered = false

    var body: some View {
        if node.status == "online" || node.status == "device" {
            connectedCard
        } else {
            disconnectedCard
        }
    }

    // MARK: - Connected Node Card

    private var connectedCard: some View {
        HStack(alignment: .top, spacing: Spacing.lg) {
            DeviceAvatar(
                platform: node.platform,
                name: node.displayName,
                isOnline: true,
                size: 56
            )

            VStack(alignment: .leading, spacing: Spacing.sm) {
                Text(node.displayName)
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundColor(.primary)

                HStack(spacing: Spacing.xs) {
                    Text(node.platformLabel)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundColor(.secondary)

                    Text("·")
                        .foregroundColor(.secondary)

                    Circle()
                        .fill(Color.brandCyan)
                        .frame(width: 6, height: 6)

                    Text("Online")
                        .font(.system(size: 12))
                        .foregroundColor(Color.brandCyan)
                }

                Spacer().frame(height: Spacing.xs)

                HStack(spacing: Spacing.sm) {
                    ForEach(availableActions.prefix(4)) { action in
                        actionButton(action.label, icon: action.icon, actionId: action.id)
                    }
                }

                if availableActions.count > 4 {
                    Divider()
                        .background(Color.primary.opacity(0.1))
                        .padding(.vertical, Spacing.xs)

                    HStack(spacing: Spacing.sm) {
                        ForEach(availableActions.dropFirst(4)) { action in
                            actionButton(action.label, icon: action.icon, actionId: action.id)
                        }
                    }
                }
            }

            Spacer()

            capabilitiesBadge
        }
        .padding(Spacing.xxl)
        .background(
            RoundedRectangle(cornerRadius: Radius.card)
                .fill(Color.surfaceCard)
                .overlay(
                    RoundedRectangle(cornerRadius: Radius.card)
                        .strokeBorder(
                            isHovered ? Color.brandCyan.opacity(0.35) : Color.clear,
                            lineWidth: 1.5
                        )
                )
                .shadow(
                    color: .black.opacity(isHovered ? Elevation.elevated.opacity : Elevation.resting.opacity),
                    radius: isHovered ? Elevation.elevated.radius : Elevation.resting.radius,
                    x: 0,
                    y: isHovered ? Elevation.elevated.y : Elevation.resting.y
                )
        )
        .scaleEffect(isHovered ? 1.02 : 1)
        .onHover { hovering in
            withAnimation(Anim.hover) { isHovered = hovering }
            if hovering { NSCursor.pointingHand.push() } else { NSCursor.pop() }
        }
        .contextMenu { nodeContextMenu }
    }

    // MARK: - Disconnected Node Card

    private var disconnectedCard: some View {
        HStack(spacing: Spacing.lg) {
            DeviceAvatar(
                platform: node.platform,
                name: node.displayName,
                isOnline: false,
                size: 48
            )

            VStack(alignment: .leading, spacing: Spacing.xs) {
                Text(node.displayName)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundColor(.primary)

                HStack(spacing: Spacing.xs) {
                    Text(node.platformLabel)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundColor(.secondary)

                    if let error = connectionError {
                        Text("· \(error)")
                            .font(.system(size: 11))
                            .foregroundColor(.red)
                    } else {
                        HStack(spacing: Spacing.xxs) {
                            Circle()
                                .stroke(style: StrokeStyle(lineWidth: 1.5, dash: [2, 3]))
                                .fill(Color.gray)
                                .frame(width: 6, height: 6)
                            Text("Not connected")
                                .font(.system(size: 12))
                                .foregroundColor(.secondary)
                        }
                    }
                }
            }

            Spacer()

            Button(action: {
                isConnecting = true
                connectionError = nil
                onConnect(node.nodeId)
            }) {
                if isConnecting {
                    ProgressView()
                        .scaleEffect(0.55)
                        .frame(width: 16, height: 16)
                } else {
                    Text("Connect")
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            .disabled(isConnecting)
        }
        .padding(Spacing.lg)
        .background(
            RoundedRectangle(cornerRadius: Radius.card)
                .fill(Color.surfaceCard)
        )
        .contextMenu { nodeContextMenu }
    }

    // MARK: - Context Menu

    private var nodeContextMenu: some View {
        Group {
            Button("Copy Link") { copyToClipboard(node.nodeId) }
            if node.status == "online" || node.status == "device" {
                Button("Ping") { onAction(node.nodeId, "ping") }
            }
            Divider()
            Button(role: .destructive, action: { onForget(node.nodeId) }) {
                Label("Remove Node", systemImage: "trash")
            }
        }
    }

    // MARK: - Action Button

    private func actionButton(_ title: String, icon: String, actionId: String) -> some View {
        Button(action: {
            isLoadingAction = actionId
            onAction(node.nodeId, actionId)
            if actionId == "clipboard" || actionId == "ping" {
                withAnimation(Anim.hover) { isLoadingAction = nil }
            } else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    withAnimation { isLoadingAction = nil }
                }
            }
        }) {
            if isLoadingAction == actionId {
                ProgressView()
                    .scaleEffect(0.55)
                    .frame(width: 16, height: 16)
                    .frame(maxWidth: .infinity)
            } else {
                HStack(spacing: Spacing.xxs) {
                    Image(systemName: icon)
                        .font(.system(size: 12))
                    Text(title)
                        .font(.system(size: 12))
                }
                .frame(maxWidth: .infinity)
            }
        }
        .buttonStyle(.bordered)
        .controlSize(.regular)
        .disabled(isLoadingAction != nil)
    }

    // MARK: - Capabilities Badge

    private var capabilitiesBadge: some View {
        Group {
            if !node.capabilities.isEmpty {
                Text("+\(node.capabilities.count)")
                    .font(.caption2)
                    .foregroundColor(.secondary)
                    .padding(.horizontal, Spacing.xs)
                    .padding(.vertical, Spacing.xxs)
                    .background(Capsule().fill(.quaternary))
                    .help(node.capabilities.map(CapabilityInfo.label).joined(separator: "\n"))
            }
        }
    }

    // MARK: - Available Actions

    private var availableActions: [NodeAction] {
        let allActions: [NodeAction] = [
            NodeAction(id: "clipboard", label: "Clipboard", icon: "doc.on.clipboard"),
            NodeAction(id: "terminal", label: "Terminal", icon: "terminal"),
            NodeAction(id: "notifications", label: "Notify", icon: "bell"),
            NodeAction(id: "file", label: "Files", icon: "doc"),
            NodeAction(id: "ping", label: "Ping", icon: "antenna.radiowaves.left.and.right"),
        ]
        if node.capabilities.isEmpty {
            return allActions
        }
        return allActions.filter { a in
            node.capabilities.contains(a.id) || a.id == "ping"
        }
    }

    private func copyToClipboard(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

// MARK: - Platform Label Extension

extension PDOSNode {
    var platformLabel: String {
        switch platform.lowercased() {
        case "macos", "mac": return "macOS"
        case "android": return "Android"
        case "linux": return "Linux"
        case "windows": return "Windows"
        default: return platform.capitalized
        }
    }
}
