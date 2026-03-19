/// MuonGit - Checkout: materialize index entries into the working directory
/// Parity: libgit2 src/libgit2/checkout.c
import Foundation

/// Options for checkout behavior.
public struct CheckoutOptions {
    /// If true, overwrite existing files in the workdir.
    public var force: Bool

    public init(force: Bool = false) {
        self.force = force
    }
}

/// Result of a checkout operation.
public struct CheckoutResult {
    /// Files written to the workdir.
    public var updated: [String] = []
    /// Files skipped because they already exist (when force is false).
    public var conflicts: [String] = []
}

/// Checkout the index to the working directory.
public func checkoutIndex(gitDir: String, workdir: String, options: CheckoutOptions) throws -> CheckoutResult {
    let index = try readIndex(gitDir: gitDir)
    var result = CheckoutResult()

    for entry in index.entries {
        try checkoutEntry(gitDir: gitDir, workdir: workdir, entry: entry, options: options, result: &result)
    }

    return result
}

/// Checkout specific paths from the index to the working directory.
public func checkoutPaths(gitDir: String, workdir: String, paths: [String], options: CheckoutOptions) throws -> CheckoutResult {
    let index = try readIndex(gitDir: gitDir)
    var result = CheckoutResult()

    for path in paths {
        guard let entry = index.find(path: path) else {
            throw MuonGitError.notFound("path '\(path)' not in index")
        }
        try checkoutEntry(gitDir: gitDir, workdir: workdir, entry: entry, options: options, result: &result)
    }

    return result
}

private func checkoutEntry(gitDir: String, workdir: String, entry: IndexEntry, options: CheckoutOptions, result: inout CheckoutResult) throws {
    let targetPath = (workdir as NSString).appendingPathComponent(entry.path)

    // Check for existing file when not forcing
    if !options.force && FileManager.default.fileExists(atPath: targetPath) {
        result.conflicts.append(entry.path)
        return
    }

    // Create parent directories
    let parentDir = (targetPath as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parentDir, withIntermediateDirectories: true)

    // Read blob content
    let blob = try readBlob(gitDir: gitDir, oid: entry.oid)

    // Write file
    try blob.data.write(to: URL(fileURLWithPath: targetPath))

    // Set file permissions based on mode
    let mode = entry.mode & 0o777
    let perms: Int = (mode & 0o111) != 0 ? 0o755 : 0o644
    try FileManager.default.setAttributes(
        [.posixPermissions: perms],
        ofItemAtPath: targetPath
    )

    result.updated.append(entry.path)
}
