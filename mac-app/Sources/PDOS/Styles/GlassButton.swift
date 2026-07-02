import SwiftUI

struct GlassButton: View {
    let systemImage: String
    let action: () -> Void
    var helpText: String = ""

    @State private var isHovering = false
    @State private var isPressed = false

    var body: some View {
        Button(action: {
            withAnimation(.spring(response: 0.15, dampingFraction: 0.5)) {
                isPressed = true
            }
            NSHapticFeedbackManager.defaultPerformer.perform(.levelChange,
                performanceTime: .default)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) {
                withAnimation(.spring(response: 0.15, dampingFraction: 0.5)) {
                    isPressed = false
                }
            }
            action()
        }) {
            Image(systemName: systemImage)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 32, height: 32)
                .background {
                    Circle()
                        .fill(.white.opacity(isHovering ? 0.15 : 0.08))
                        .background(.thickMaterial, in: Circle())
                        .overlay(
                            Circle()
                                .strokeBorder(
                                    isHovering ? Color.brandCyan.opacity(0.4) : .white.opacity(0.25),
                                    lineWidth: isHovering ? 1.5 : 0.5
                                )
                        )
                }
                .shadow(color: .black.opacity(isHovering ? Elevation.elevated.opacity : Elevation.resting.opacity), radius: isHovering ? Elevation.elevated.radius : Elevation.resting.radius, x: 0, y: isHovering ? Elevation.elevated.y : Elevation.resting.y)
                .scaleEffect(isPressed ? 0.85 : 1.0)
        }
        .buttonStyle(.plain)
        .help(helpText)
        .onHover { hovering in
            withAnimation(.easeInOut(duration: 0.2)) {
                isHovering = hovering
            }
        }
    }
}
