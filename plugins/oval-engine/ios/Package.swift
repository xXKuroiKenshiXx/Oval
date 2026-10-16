// swift-tools-version:5.5

import PackageDescription

let package = Package(
    name: "tauri-plugin-oval-engine",
    platforms: [
        .macOS(.v10_13),
        .iOS(.v13),
    ],
    products: [
        .library(
            name: "tauri-plugin-oval-engine",
            type: .static,
            targets: ["tauri-plugin-oval-engine"])
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "tauri-plugin-oval-engine",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
