import SwiftUI

/// The successful pairing state — shows device info and connect button.
struct PairedDeviceView: View {
    let device: PairedDevice
    let onConnect: (String, Int) -> Void

    var body: some View {
        VStack(spacing: Spacing.xl) {
            Spacer()

            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 64))
                .foregroundColor(.green)

            Text("Device Paired")
                .font(.title)
                .fontWeight(.bold)
                .foregroundColor(.white)

            deviceInfoCard

            if device.platform == "mac", let url = device.relayURL {
                connectButton(relayURL: url)
            }

            Spacer()
        }
        .padding(Spacing.xxl)
    }

    private var deviceInfoCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            detailRow(label: "Name", value: device.name)
            detailRow(label: "Platform", value: device.platform.capitalized)
            if let url = device.relayURL {
                detailRow(label: "Relay", value: url)
            }
        }
        .padding(Spacing.lg)
        .background(.ultraThinMaterial)
        .cornerRadius(Radius.card)
    }

    private func connectButton(relayURL: String) -> some View {
        Button {
            let hostPort = relayURL
                .replacingOccurrences(of: "ws://", with: "")
                .replacingOccurrences(of: "wss://", with: "")
            let parts = hostPort.split(separator: ":")
            if parts.count == 2, let port = Int(parts[1]) {
                onConnect(String(parts[0]), port)
            }
        } label: {
            Text("Connect to Device")
                .fontWeight(.semibold)
                .frame(maxWidth: .infinity)
                .frame(height: 36)
        }
        .buttonStyle(.borderedProminent)
        .padding(.horizontal, Spacing.xxl)
    }

    private func detailRow(label: String, value: String) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundColor(.secondary)
                .frame(width: 70, alignment: .trailing)
            Text(value)
                .font(.subheadline)
                .foregroundColor(.white)
            Spacer()
        }
    }
}
