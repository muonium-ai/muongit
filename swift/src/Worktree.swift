// Git worktree support — multiple working trees for a single repository.
// Parity: libgit2 src/libgit2/worktree.c

import Foundation

/// A linked worktree entry.
public struct Worktree: Sendable, Equatable {
    /// Name of the worktree (basename under .git/worktrees/).
    public let name: String
    /// Filesystem path to the worktree working directory.
    public let path: String
    /// Path to the worktree's gitdir inside the parent's .git/worktrees/<name>/.
    public let gitdirPath: String
    /// Whether this worktree is locked.
    public let locked: Bool
}

/// Options for creating a new worktree.
public struct WorktreeAddOptions: Sendable {
    /// Lock the newly created worktree immediately.
    public var lock: Bool
    /// Branch reference (e.g. "refs/heads/feature"). If nil, creates a new
    /// branch named after the worktree pointing at HEAD.
    public var reference: String?

    public init(lock: Bool = false, reference: String? = nil) {
        self.lock = lock
        self.reference = reference
    }
}

/// Options controlling worktree prune behavior.
public struct WorktreePruneOptions: Sendable {
    /// Prune even if the worktree is valid (on-disk data exists).
    public var valid: Bool
    /// Prune even if the worktree is locked.
    public var locked: Bool
    /// Also remove the working tree directory.
    public var workingTree: Bool

    public init(valid: Bool = false, locked: Bool = false, workingTree: Bool = false) {
        self.valid = valid
        self.locked = locked
        self.workingTree = workingTree
    }
}

// MARK: - Public API

/// List names of linked worktrees for a repository.
public func worktreeList(gitDir: String) throws -> [String] {
    let fm = FileManager.default
    let worktreesDir = (gitDir as NSString).appendingPathComponent("worktrees")
    var isDir: ObjCBool = false
    guard fm.fileExists(atPath: worktreesDir, isDirectory: &isDir), isDir.boolValue else {
        return []
    }

    let entries = try fm.contentsOfDirectory(atPath: worktreesDir)
    var names: [String] = []
    for entry in entries {
        let entryPath = (worktreesDir as NSString).appendingPathComponent(entry)
        var entryIsDir: ObjCBool = false
        if fm.fileExists(atPath: entryPath, isDirectory: &entryIsDir),
           entryIsDir.boolValue,
           isWorktreeDir(entryPath) {
            names.append(entry)
        }
    }
    return names.sorted()
}

/// Look up a linked worktree by name.
public func worktreeLookup(gitDir: String, name: String) throws -> Worktree {
    let fm = FileManager.default
    let wtDir = (gitDir as NSString).appendingPathComponent("worktrees/\(name)")
    var isDir: ObjCBool = false
    guard fm.fileExists(atPath: wtDir, isDirectory: &isDir), isDir.boolValue else {
        throw MuonGitError.notFound("worktree '\(name)' not found")
    }
    guard isWorktreeDir(wtDir) else {
        throw MuonGitError.invalid("worktree '\(name)' has invalid structure")
    }
    return try openWorktree(gitDir: gitDir, name: name)
}

/// Validate that a worktree's on-disk structure is intact.
public func worktreeValidate(worktree: Worktree) throws {
    let fm = FileManager.default
    var isDir: ObjCBool = false

    guard fm.fileExists(atPath: worktree.gitdirPath, isDirectory: &isDir), isDir.boolValue else {
        throw MuonGitError.notFound("worktree gitdir missing: \(worktree.gitdirPath)")
    }
    guard isWorktreeDir(worktree.gitdirPath) else {
        throw MuonGitError.invalid("worktree '\(worktree.name)' has invalid gitdir structure")
    }
    guard fm.fileExists(atPath: worktree.path, isDirectory: &isDir), isDir.boolValue else {
        throw MuonGitError.notFound("worktree working directory missing: \(worktree.path)")
    }
}

/// Add a new linked worktree.
public func worktreeAdd(
    gitDir: String,
    name: String,
    worktreePath: String,
    options: WorktreeAddOptions = WorktreeAddOptions()
) throws -> Worktree {
    let fm = FileManager.default
    let wtMeta = (gitDir as NSString).appendingPathComponent("worktrees/\(name)")
    guard !fm.fileExists(atPath: wtMeta) else {
        throw MuonGitError.conflict("worktree '\(name)' already exists")
    }

    // Determine the branch ref
    let branchRef: String
    if let ref = options.reference {
        branchRef = ref
    } else {
        let headOid = try resolveReference(gitDir: gitDir, name: "HEAD")
        let newBranch = "refs/heads/\(name)"
        try writeReference(gitDir: gitDir, name: newBranch, oid: headOid)
        branchRef = newBranch
    }

    // Create metadata dir and worktree dir
    try fm.createDirectory(atPath: wtMeta, withIntermediateDirectories: true)
    try fm.createDirectory(atPath: worktreePath, withIntermediateDirectories: true)

    let absWorktree: String
    do {
        absWorktree = try resolveRealPath(worktreePath)
    }

    // Write gitdir file (points to worktree's .git file)
    let gitfileInWt = (absWorktree as NSString).appendingPathComponent(".git")
    try (gitfileInWt + "\n").write(
        toFile: (wtMeta as NSString).appendingPathComponent("gitdir"),
        atomically: true, encoding: .utf8)

    // Write commondir file
    try "../..\n".write(
        toFile: (wtMeta as NSString).appendingPathComponent("commondir"),
        atomically: true, encoding: .utf8)

    // Write HEAD as symbolic ref
    try "ref: \(branchRef)\n".write(
        toFile: (wtMeta as NSString).appendingPathComponent("HEAD"),
        atomically: true, encoding: .utf8)

    // Create .git file in worktree (gitlink pointing back to metadata)
    let absWtMeta = try resolveRealPath(wtMeta)
    try "gitdir: \(absWtMeta)\n".write(
        toFile: gitfileInWt, atomically: true, encoding: .utf8)

    // Lock if requested
    if options.lock {
        try "".write(
            toFile: (wtMeta as NSString).appendingPathComponent("locked"),
            atomically: true, encoding: .utf8)
    }

    return Worktree(
        name: name,
        path: absWorktree,
        gitdirPath: absWtMeta,
        locked: options.lock
    )
}

/// Lock a worktree with an optional reason.
public func worktreeLock(gitDir: String, name: String, reason: String? = nil) throws {
    let fm = FileManager.default
    let wtMeta = (gitDir as NSString).appendingPathComponent("worktrees/\(name)")
    guard fm.fileExists(atPath: wtMeta) else {
        throw MuonGitError.notFound("worktree '\(name)' not found")
    }
    let lockPath = (wtMeta as NSString).appendingPathComponent("locked")
    guard !fm.fileExists(atPath: lockPath) else {
        throw MuonGitError.locked("worktree '\(name)' is already locked")
    }
    try (reason ?? "").write(toFile: lockPath, atomically: true, encoding: .utf8)
}

/// Unlock a worktree. Returns true if was locked, false if was not.
public func worktreeUnlock(gitDir: String, name: String) throws -> Bool {
    let fm = FileManager.default
    let lockPath = (gitDir as NSString).appendingPathComponent("worktrees/\(name)/locked")
    if fm.fileExists(atPath: lockPath) {
        try fm.removeItem(atPath: lockPath)
        return true
    }
    return false
}

/// Check whether a worktree is locked. Returns the lock reason if locked, nil otherwise.
public func worktreeIsLocked(gitDir: String, name: String) throws -> String? {
    let fm = FileManager.default
    let lockPath = (gitDir as NSString).appendingPathComponent("worktrees/\(name)/locked")
    guard fm.fileExists(atPath: lockPath) else { return nil }
    let reason = try String(contentsOfFile: lockPath, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
    return reason
}

/// Check if a worktree can be pruned with the given options.
public func worktreeIsPrunable(
    gitDir: String,
    name: String,
    options: WorktreePruneOptions = WorktreePruneOptions()
) throws -> Bool {
    let wt = try worktreeLookup(gitDir: gitDir, name: name)
    let fm = FileManager.default

    if wt.locked && !options.locked { return false }

    var isDir: ObjCBool = false
    if fm.fileExists(atPath: wt.path, isDirectory: &isDir), isDir.boolValue, !options.valid {
        return false
    }

    return true
}

/// Prune (remove) a worktree's metadata. Optionally removes the working directory.
public func worktreePrune(
    gitDir: String,
    name: String,
    options: WorktreePruneOptions = WorktreePruneOptions()
) throws {
    let wt = try worktreeLookup(gitDir: gitDir, name: name)
    let fm = FileManager.default

    if wt.locked && !options.locked {
        throw MuonGitError.locked("worktree '\(name)' is locked")
    }

    var isDir: ObjCBool = false
    if fm.fileExists(atPath: wt.path, isDirectory: &isDir), isDir.boolValue, !options.valid {
        throw MuonGitError.conflict("worktree '\(name)' is still valid; use valid flag to override")
    }

    // Remove working tree directory if requested
    if options.workingTree, fm.fileExists(atPath: wt.path) {
        try fm.removeItem(atPath: wt.path)
    }

    // Remove metadata directory
    let wtMeta = (gitDir as NSString).appendingPathComponent("worktrees/\(name)")
    if fm.fileExists(atPath: wtMeta) {
        try fm.removeItem(atPath: wtMeta)
    }

    // Clean up worktrees dir if empty
    let worktreesDir = (gitDir as NSString).appendingPathComponent("worktrees")
    if let remaining = try? fm.contentsOfDirectory(atPath: worktreesDir), remaining.isEmpty {
        try? fm.removeItem(atPath: worktreesDir)
    }
}

// MARK: - Internal helpers

private func isWorktreeDir(_ path: String) -> Bool {
    let fm = FileManager.default
    return fm.fileExists(atPath: (path as NSString).appendingPathComponent("gitdir"))
        && fm.fileExists(atPath: (path as NSString).appendingPathComponent("commondir"))
        && fm.fileExists(atPath: (path as NSString).appendingPathComponent("HEAD"))
}

private func openWorktree(gitDir: String, name: String) throws -> Worktree {
    let fm = FileManager.default
    let wtDir = (gitDir as NSString).appendingPathComponent("worktrees/\(name)")

    let gitdirContent = try String(contentsOfFile:
        (wtDir as NSString).appendingPathComponent("gitdir"), encoding: .utf8)
        .trimmingCharacters(in: .whitespacesAndNewlines)

    // The worktree path is the parent of the .git file referenced in gitdir
    let worktreePath = (gitdirContent as NSString).deletingLastPathComponent

    let lockPath = (wtDir as NSString).appendingPathComponent("locked")
    let locked = fm.fileExists(atPath: lockPath)

    return Worktree(
        name: name,
        path: worktreePath,
        gitdirPath: wtDir,
        locked: locked
    )
}

private func resolveRealPath(_ path: String) throws -> String {
    let fm = FileManager.default
    // Use URL-based resolution for real path
    let url = URL(fileURLWithPath: path).standardized
    // Try to resolve symlinks via FileManager
    if let resolved = try? fm.destinationOfSymbolicLink(atPath: path) {
        return resolved
    }
    // Fall back to realpath(3)
    if let real = (path as NSString).resolvingSymlinksInPath as String? {
        return real
    }
    return url.path
}
