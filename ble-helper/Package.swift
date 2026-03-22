// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "surfterm-ble-helper",
    platforms: [.macOS(.v12)],
    targets: [
        .executableTarget(name: "surfterm-ble-helper", path: "Sources")
    ]
)
