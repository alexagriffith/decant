// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "decant-menubar",
    platforms: [.macOS(.v14)],
    targets: [
        // Tested core: config discovery, daemon client, SSE, view-state store.
        .target(name: "DecantKit"),
        // Thin SwiftUI MenuBarExtra shell over DecantKit.
        .executableTarget(name: "DecantBar", dependencies: ["DecantKit"]),
        .testTarget(name: "DecantKitTests", dependencies: ["DecantKit"]),
    ]
)
