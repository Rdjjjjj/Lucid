// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "EnglishInput",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "EnglishInputCore", targets: ["EnglishInputCore"]),
        .library(name: "EnglishInputMethod", targets: ["EnglishInputMethod"]),
    ],
    targets: [
        .target(name: "EnglishInputCore"),
        .target(
            name: "EnglishInputMethod",
            dependencies: ["EnglishInputCore"],
            linkerSettings: [
                .linkedFramework("InputMethodKit"),
                .linkedFramework("AppKit"),
            ]
        ),
        .testTarget(name: "EnglishInputCoreTests", dependencies: ["EnglishInputCore"]),
    ]
)
