import SwiftUI

/// The active pairing state — shows QR code, connection info, and controls.
struct PairingActiveView: View {
    @ObservedObject var pairingService: QRPairingService
    let session: PairingSession
    let qrImage: NSImage?
    let onRegenerate: () -> Void
    let onStop: () -> Void

    @State private var showingCopied = false
    @State private var ipIndex = 0

    var body: some View {
        VStack(spacing: Spacing.xl) {
            headerBar
            Spacer()
            instructionText
            QRCodeRegionView(qrImage: qrImage)
            connectionInfoSection
            Spacer()
            pairedBanner
            footerButtons
        }
        .padding(Spacing.xxl)
    }

    // MARK: - Subviews

    private var headerBar: some View {
        HStack {
            Image(systemName: "qrcode.viewfinder")
                .font(.title3)
                .foregroundColor(.brandCyan)
            Text("QR Pairing")
                .font(.headline)
                .foregroundColor(.white)
            Spacer()
            HStack(spacing: 4) {
                Circle()
                    .fill(pairingService.peerPaired ? Color.green : Color.brandCyan)
                    .frame(width: 8, height: 8)
                Text(pairingService.peerPaired ? "Paired" : "Listening")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
        }
    }

    private var instructionText: some View {
        VStack(spacing: Spacing.lg) {
            Text("Scan to Connect")
                .font(.system(size: 22, weight: .bold))
                .foregroundColor(.white)
            Text("Point your phone camera at this screen")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
    }

    private var connectionInfoSection: some View {
        VStack(spacing: Spacing.sm) {
            HStack(spacing: 6) {
                Circle()
                    .fill(Color.green)
                    .frame(width: 6, height: 6)
                Text("Pairing active — port \(session.port)")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }

            HStack(spacing: 4) {
                Text(session.ipAddresses[safe: ipIndex] ?? session.ipAddresses.first ?? "")
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.secondary)
                    .onTapGesture {
                        withAnimation {
                            ipIndex = (ipIndex + 1) % max(session.ipAddresses.count, 1)
                        }
                    }

                if session.ipAddresses.count > 1 {
                    Image(systemName: "arrow.triangle.2.circlepath")
                        .font(.caption2)
                        .foregroundColor(.secondary.opacity(0.5))
                }

                Button {
                    copyToClipboard(session.qrPayload)
                    showingCopied = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                        showingCopied = false
                    }
                } label: {
                    Image(systemName: showingCopied ? "checkmark" : "doc.on.doc")
                        .font(.caption2)
                        .foregroundColor(showingCopied ? .green : .secondary)
                }
                .buttonStyle(.plain)
                .help("Copy pairing URL")
            }

            if showingCopied {
                Text("Copied!")
                    .font(.caption2)
                    .foregroundColor(.green)
                    .transition(.opacity)
            }
        }
    }

    @ViewBuilder
    private var pairedBanner: some View {
        if let device = pairingService.pairedDevice {
            Group {
                HStack(spacing: 12) {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundColor(.green)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Paired with \(device.name)")
                            .font(.subheadline).fontWeight(.semibold)
                            .foregroundColor(.white)
                        Text(device.platform.capitalized)
                            .font(.caption).foregroundColor(.secondary)
                    }
                    Spacer()
                }
                .padding(Spacing.lg)
                .background(.ultraThinMaterial)
                .cornerRadius(Radius.card)
            }
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }

    private var footerButtons: some View {
        HStack(spacing: Spacing.md) {
            if !pairingService.peerPaired {
                Button {
                    onRegenerate()
                } label: {
                    Label("Regenerate", systemImage: "arrow.clockwise")
                        .font(.caption)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
            Button(role: .destructive) {
                onStop()
            } label: {
                Label("Stop", systemImage: "stop.fill")
                    .font(.caption)
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
    }

    private func copyToClipboard(_ string: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(string, forType: .string)
    }
}
