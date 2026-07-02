import SwiftUI

/// The inactive (pre-pairing) state — prompts user to start a session.
struct PairingInactiveView: View {
    let onStart: () -> Void

    var body: some View {
        VStack(spacing: Spacing.xl) {
            Spacer()

            Image(systemName: "qrcode")
                .font(.system(size: 48))
                .foregroundColor(.secondary)

            Text("QR Pairing")
                .font(.title2)
                .fontWeight(.semibold)
                .foregroundColor(.white)

            Text("Start a pairing session to connect your phone")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)

            Button {
                onStart()
            } label: {
                Label("Start Pairing", systemImage: "play.fill")
                    .fontWeight(.semibold)
                    .frame(maxWidth: .infinity)
                    .frame(height: 36)
            }
            .buttonStyle(.borderedProminent)

            Spacer()
        }
        .padding(Spacing.xxl)
    }
}
