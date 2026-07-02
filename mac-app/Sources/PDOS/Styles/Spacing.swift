import SwiftUI

enum Spacing {
    static let xxs: CGFloat = 2
    static let xs: CGFloat = 4
    static let sm: CGFloat = 8
    static let md: CGFloat = 12
    static let lg: CGFloat = 16
    static let xl: CGFloat = 20
    static let xxl: CGFloat = 24
    static let xxxl: CGFloat = 32
}

enum Radius {
    static let card: CGFloat = 12
    static let sm: CGFloat = 8
    static let full: CGFloat = 999
}

enum Elevation {
    static let resting: (radius: CGFloat, y: CGFloat, opacity: Double) = (4, 1, 0.06)
    static let elevated: (radius: CGFloat, y: CGFloat, opacity: Double) = (12, 4, 0.15)
}
