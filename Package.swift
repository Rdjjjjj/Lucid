// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Lucid",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "LucidCore", targets: ["LucidCore"]),
    ],
    targets: [
        .target(name: "LucidCore", exclude: ["Info.plist"]),
        .testTarget(name: "LucidCoreTests", dependencies: ["LucidCore"]),
    ]
)
