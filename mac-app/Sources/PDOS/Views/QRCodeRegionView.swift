import SwiftUI

/// Displays the QR code image with a glowing border and breathing animation.
struct QRCodeRegionView: View {
    let qrImage: NSImage?

    @State private var glowOpacity: CGFloat = 0.3
    @State private var breathingScale: CGFloat = 1.0

    var body: some View {
        ZStack {
            if let image = qrImage {
                Image(nsImage: image)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
                    .frame(width: 260, height: 260)
                    .cornerRadius(Radius.card)
                    .overlay(
                        RoundedRectangle(cornerRadius: Radius.card)
                            .strokeBorder(Color.brandCyan.opacity(glowOpacity), lineWidth: 2)
                    )
                    .shadow(
                        color: Color.brandCyan.opacity(glowOpacity * 0.5),
                        radius: 20, x: 0, y: 0
                    )
                    .scaleEffect(breathingScale)
            } else {
                ProgressView()
                    .scaleEffect(1.5)
                    .frame(width: 260, height: 260)
            }

            centerBadge
        }
        .onAppear(perform: startAnimations)
    }

    private var centerBadge: some View {
        ZStack {
            Circle()
                .fill(Color.black.opacity(0.8))
                .frame(width: 50, height: 50)
            Image(systemName: "antenna.radiowaves.left.and.right")
                .font(.system(size: 20, weight: .bold))
                .foregroundColor(.brandCyan)
        }
    }

    private func startAnimations() {
        withAnimation(.easeInOut(duration: 2.5).repeatForever(autoreverses: true)) {
            breathingScale = 1.03
        }
        withAnimation(.easeInOut(duration: 2).repeatForever(autoreverses: true)) {
            glowOpacity = 0.6
        }
    }
}
