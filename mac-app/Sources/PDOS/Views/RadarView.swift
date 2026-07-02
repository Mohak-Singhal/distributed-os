import SwiftUI

/// AirDrop-style radar scanner view with pulsing rings and sweep animation.
struct RadarView: View {
    let isScanning: Bool
    let deviceCount: Int

    @State private var sweepAngle: Double = 0
    @State private var pulsePhase: Double = 0
    @State private var ringOpacities: [Double] = [0.4, 0.3, 0.2]
    @State private var sweepScale: CGFloat = 1.0

    private let ringCount = 3
    private let scanDuration: Double = 3.0

    var body: some View {
        ZStack {
            // Background subtle glow
            Circle()
                .fill(
                    RadialGradient(
                        colors: [
                            Color.brandCyan.opacity(isScanning ? 0.08 : 0.02),
                            Color.clear
                        ],
                        center: .center,
                        startRadius: 0,
                        endRadius: 140
                    )
                )
                .frame(width: 280, height: 280)
                .animation(.easeInOut(duration: 0.5), value: isScanning)

            // Pulsing rings
            ForEach(0..<ringCount, id: \.self) { index in
                Circle()
                    .stroke(
                        Color.brandCyan.opacity(ringOpacities[index]),
                        lineWidth: 1.5
                    )
                    .frame(
                        width: ringSize(for: index),
                        height: ringSize(for: index)
                    )
            }

            // Sweep cone
            if isScanning {
                GeometryReader { geo in
                    let center = CGPoint(x: geo.size.width / 2, y: geo.size.height / 2)
                    let radius = min(geo.size.width, geo.size.height) / 2

                    Path { path in
                        path.move(to: center)
                        path.addArc(
                            center: center,
                            radius: radius,
                            startAngle: .degrees(sweepAngle - 15),
                            endAngle: .degrees(sweepAngle + 15),
                            clockwise: false
                        )
                        path.closeSubpath()
                    }
                    .fill(
                        AngularGradient(
                            colors: [
                                Color.brandCyan.opacity(0.3),
                                Color.brandCyan.opacity(0.05),
                                Color.clear
                            ],
                            center: .center,
                            startAngle: .degrees(sweepAngle - 20),
                            endAngle: .degrees(sweepAngle + 20)
                        )
                    )
                    .blendMode(.screen)
                }
                .frame(width: 280, height: 280)
                .scaleEffect(sweepScale)
            }

            // Center dot
            Circle()
                .fill(Color.brandCyan)
                .frame(width: 8, height: 8)
                .shadow(color: Color.brandCyan.opacity(0.6), radius: 6)

            // Device count badge
            if deviceCount > 0 {
                VStack {
                    Spacer()
                    HStack {
                        Spacer()
                        Text("\(deviceCount)")
                            .font(.system(size: 11, weight: .bold))
                            .foregroundColor(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 3)
                            .background(Capsule().fill(Color.brandCyan))
                            .offset(x: -20, y: -20)
                    }
                }
                .frame(width: 280, height: 280)
            }
        }
        .onAppear {
            if isScanning {
                startAnimations()
            }
        }
        .onChange(of: isScanning) { _, scanning in
            if scanning {
                startAnimations()
            } else {
                stopAnimations()
            }
        }
    }

    private func ringSize(for index: Int) -> CGFloat {
        let base: CGFloat = 80
        let spacing: CGFloat = 40
        return base + CGFloat(index) * spacing
    }

    private func startAnimations() {
        // Sweep rotation
        withAnimation(.linear(duration: scanDuration).repeatForever(autoreverses: false)) {
            sweepAngle = 360
        }

        // Ring pulse
        withAnimation(.easeInOut(duration: 1.8).repeatForever(autoreverses: true)) {
            ringOpacities = [0.5, 0.4, 0.3]
        }

        // Subtle scale pulse
        withAnimation(.easeInOut(duration: 1.5).repeatForever(autoreverses: true)) {
            sweepScale = 1.02
        }
    }

    private func stopAnimations() {
        sweepAngle = 0
        ringOpacities = [0.4, 0.3, 0.2]
        sweepScale = 1.0
    }
}

#Preview {
    ZStack {
        Color.black
        RadarView(isScanning: true, deviceCount: 2)
    }
    .frame(width: 300, height: 300)
}
