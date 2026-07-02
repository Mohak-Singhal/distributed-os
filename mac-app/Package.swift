// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PDOS",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "PDOS",
            dependencies: [],
            resources: [],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .executableTarget(
            name: "PDOSShareExtension",
            dependencies: [],
            path: "Sources/PDOSShareExtension",
            resources: [],
            swiftSettings: [.swiftLanguageMode(.v5)]
        )
    ]
)
