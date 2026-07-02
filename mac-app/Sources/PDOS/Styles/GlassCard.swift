import SwiftUI

enum Anim {
    static let press = Animation.spring(response: 0.15, dampingFraction: 0.5)
    static let hover = Animation.easeInOut(duration: 0.18)
    static let fadeIn = Animation.spring(duration: 0.5, bounce: 0.12)
    static let tabSwitch = Animation.spring(duration: 0.35, bounce: 0.18)
    static let reveal = Animation.spring(duration: 0.4, bounce: 0.1)
    static let progress = Animation.interpolatingSpring(duration: 0.3)
}

struct GlassCard<Content: View>: View {
    let content: Content
    var delay: Double = 0

    @State private var isVisible = false
    @State private var hasAppeared = false

    init(delay: Double = 0, @ViewBuilder content: () -> Content) {
        self.delay = delay
        self.content = content()
    }

    var body: some View {
        content
            .padding()
            .background(.ultraThinMaterial)
            .clipShape(RoundedRectangle(cornerRadius: Radius.card))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.card)
                    .strokeBorder(.white.opacity(0.12), lineWidth: 0.5)
            )
            .shadow(color: .black.opacity(Elevation.resting.opacity), radius: Elevation.resting.radius, x: 0, y: Elevation.resting.y)
            .opacity(isVisible ? 1 : 0)
            .offset(y: isVisible ? 0 : 12)
            .onAppear {
                guard !hasAppeared else { return }
                hasAppeared = true
                withAnimation(Anim.fadeIn.delay(delay)) {
                    isVisible = true
                }
            }
    }
}

// MARK: - Shimmer Modifier

struct ShimmerModifier: ViewModifier {
    var tint: Color = .white.opacity(0.08)
    var highlight: Color = .white.opacity(0.18)
    var blur: CGFloat = 4

    @State private var phase: CGFloat = -1

    func body(content: Content) -> some View {
        content
            .overlay(
                GeometryReader { geo in
                    LinearGradient(
                        colors: [tint, highlight, tint],
                        startPoint: .leading,
                        endPoint: .trailing
                    )
                    .offset(x: geo.size.width * phase)
                    .blur(radius: blur)
                    .mask(content)
                }
            )
            .onAppear {
                withAnimation(.linear(duration: 1.5).repeatForever(autoreverses: false)) {
                    phase = 1
                }
            }
    }
}

extension View {
    func shimmer(tint: Color = .white.opacity(0.06), highlight: Color = .white.opacity(0.15), blur: CGFloat = 4) -> some View {
        modifier(ShimmerModifier(tint: tint, highlight: highlight, blur: blur))
    }
}

// MARK: - GlassCard Skeleton

struct GlassCardSkeleton: View {
    var body: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: 12) {
                RoundedRectangle(cornerRadius: 4)
                    .fill(.quaternary)
                    .frame(width: 80, height: 14)
                RoundedRectangle(cornerRadius: 4)
                    .fill(.quaternary)
                    .frame(height: 12)
                RoundedRectangle(cornerRadius: 4)
                    .fill(.quaternary)
                    .frame(height: 12)
                RoundedRectangle(cornerRadius: 4)
                    .fill(.quaternary)
                    .frame(width: 120, height: 12)
            }
            .shimmer()
        }
    }
}

// MARK: - ActionTile (unchanged)

struct ActionTile: View {
    let icon: String
    let title: String
    var isActive: Bool = false
    let action: () -> Void

    @State private var isHovering = false
    @State private var isPressed = false

    var body: some View {
        Button(action: {
            withAnimation(Anim.press) { isPressed = true }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                withAnimation(Anim.press) { isPressed = false }
            }
            action()
        }) {
            VStack(spacing: 6) {
                ZStack {
                    Circle()
                        .fill(
                            isActive ? Color.white :
                                (isHovering ? Color.primary.opacity(0.18) : Color.primary.opacity(0.12))
                        )
                        .frame(width: 44, height: 44)
                        .scaleEffect(isPressed ? 0.88 : 1.0)
                        .overlay(
                            Circle()
                                .strokeBorder(
                                    isHovering ? Color.cyan.opacity(0.3) : Color.clear,
                                    lineWidth: 1.5
                                )
                        )

                    Image(systemName: icon)
                        .font(.system(size: 16, weight: .medium))
                        .foregroundColor(
                            isActive ? Color.black : Color(red: 169/255, green: 169/255, blue: 169/255)
                        )
                }

                Text(title)
                    .font(.system(size: 10, weight: .bold))
                    .foregroundColor(.primary)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering in
            withAnimation(Anim.hover) { isHovering = hovering }
        }
        .scaleEffect(isPressed ? 0.95 : 1.0)
    }
}
