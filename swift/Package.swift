// swift-tools-version: 5.9
// muongit-swift: Native Swift port of libgit2

import PackageDescription

let package = Package(
    name: "muongit-swift",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
        .watchOS(.v9),
        .tvOS(.v16)
    ],
    products: [
        .library(
            name: "MuonGit",
            targets: ["MuonGit"]
        )
    ],
    targets: [
        .target(
            name: "MuonGit",
            path: "src"
        ),
        .testTarget(
            name: "MuonGitTests",
            dependencies: ["MuonGit"],
            path: "tests"
        )
    ]
)
