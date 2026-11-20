// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Vitals",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "Vitals", targets: ["Vitals"])
    ],
    targets: [
        .target(name: "CSMC"),
        .target(name: "VitalsCore", dependencies: ["CSMC"]),
        .executableTarget(name: "Vitals", dependencies: ["VitalsCore"]),
    ]
)
