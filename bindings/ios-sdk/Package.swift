// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "PirateWalletSDK",
    platforms: [
        .iOS(.v15),
    ],
    products: [
        .library(
            name: "PirateWalletSDK",
            targets: ["PirateWalletSDK"]
        ),
    ],
    targets: [
        .binaryTarget(
            name: "PirateWalletNative",
            path: "Frameworks/PirateWalletNative.xcframework"
        ),
        .target(
            name: "PirateWalletSDK",
            dependencies: ["PirateWalletNative"],
            path: "Sources/PirateWalletSDK"
        ),
        .testTarget(
            name: "PirateWalletSDKTests",
            dependencies: ["PirateWalletSDK"],
            path: "Tests/PirateWalletSDKTests"
        ),
    ]
)
