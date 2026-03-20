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

public struct SwitchOptions: Sendable, Equatable {
    public var force: Bool

    public init(force: Bool = false) {
        self.force = force
    }
}

public struct SwitchResult: Sendable, Equatable {
    public let previousHead: OID?
    public let headOID: OID
    public let headRef: String?
    public let updatedPaths: [String]
    public let removedPaths: [String]

    public init(
        previousHead: OID?,
        headOID: OID,
        headRef: String?,
        updatedPaths: [String],
        removedPaths: [String]
    ) {
        self.previousHead = previousHead
        self.headOID = headOID
        self.headRef = headRef
        self.updatedPaths = updatedPaths
        self.removedPaths = removedPaths
    }
}

public enum ResetMode: Sendable {
    case soft
    case mixed
    case hard
}

public struct ResetResult: Sendable, Equatable {
    public let previousHead: OID
    public let headOID: OID
    public let movedRef: String?
    public let updatedPaths: [String]
    public let removedPaths: [String]

    public init(
        previousHead: OID,
        headOID: OID,
        movedRef: String?,
        updatedPaths: [String],
        removedPaths: [String]
    ) {
        self.previousHead = previousHead
        self.headOID = headOID
        self.movedRef = movedRef
        self.updatedPaths = updatedPaths
        self.removedPaths = removedPaths
    }
}

public struct RestoreOptions: Sendable, Equatable {
    public var source: String?
    public var staged: Bool
    public var worktree: Bool

    public init(source: String? = nil, staged: Bool = false, worktree: Bool = true) {
        self.source = source
        self.staged = staged
        self.worktree = worktree
    }
}

public struct RestoreResult: Sendable, Equatable {
    public var stagedPaths: [String] = []
    public var removedFromIndex: [String] = []
    public var restoredPaths: [String] = []
    public var removedFromWorkdir: [String] = []

    public init(
        stagedPaths: [String] = [],
        removedFromIndex: [String] = [],
        restoredPaths: [String] = [],
        removedFromWorkdir: [String] = []
    ) {
        self.stagedPaths = stagedPaths
        self.removedFromIndex = removedFromIndex
        self.restoredPaths = restoredPaths
        self.removedFromWorkdir = removedFromWorkdir
    }
}

private struct WorkdirUpdate {
    var updatedPaths: [String] = []
    var removedPaths: [String] = []
}

private struct MaterializedEntry {
    let oid: OID
    let mode: UInt32
    let data: Data
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

public func switchBranch(
    gitDir: String,
    workdir: String,
    name: String,
    options: SwitchOptions
) throws -> SwitchResult {
    let branch = try lookupBranch(gitDir: gitDir, name: name, kind: .local)
    guard let targetOID = branch.target else {
        throw MuonGitError.invalidSpec("branch '\(name)' has no target commit")
    }

    let currentIndex = try readIndex(gitDir: gitDir)
    let previousHead = try? currentHeadOID(gitDir: gitDir)
    let currentDesc = try describeHead(gitDir: gitDir)
    let targetEntries = try materializeCommitTree(gitDir: gitDir, commitOID: targetOID)

    if !options.force {
        let conflicts = try collectSwitchConflicts(
            gitDir: gitDir,
            workdir: workdir,
            currentIndex: currentIndex,
            targetEntries: targetEntries
        )
        if !conflicts.isEmpty {
            throw MuonGitError.conflict("checkout would overwrite local changes: \(conflicts.joined(separator: ", "))")
        }
    }

    try writeIndex(gitDir: gitDir, index: indexFromMaterialized(targetEntries))
    let update = try applyWorkdirTree(workdir: workdir, currentIndex: currentIndex, targetEntries: targetEntries)
    try writeSymbolicReference(gitDir: gitDir, name: "HEAD", target: branch.referenceName)

    let oldOID = previousHead ?? OID.zero
    try appendReflog(
        gitDir: gitDir,
        refName: "HEAD",
        oldOid: oldOID,
        newOid: targetOID,
        committer: defaultSignature(),
        message: "checkout: moving from \(currentDesc) to \(name)"
    )

    return SwitchResult(
        previousHead: previousHead,
        headOID: targetOID,
        headRef: branch.referenceName,
        updatedPaths: update.updatedPaths,
        removedPaths: update.removedPaths
    )
}

public func checkoutRevision(
    gitDir: String,
    workdir: String,
    spec: String,
    options: SwitchOptions
) throws -> SwitchResult {
    let targetOID = try resolveRevision(gitDir: gitDir, spec: spec)
    let currentIndex = try readIndex(gitDir: gitDir)
    let previousHead = try? currentHeadOID(gitDir: gitDir)
    let currentDesc = try describeHead(gitDir: gitDir)
    let targetEntries = try materializeCommitTree(gitDir: gitDir, commitOID: targetOID)

    if !options.force {
        let conflicts = try collectSwitchConflicts(
            gitDir: gitDir,
            workdir: workdir,
            currentIndex: currentIndex,
            targetEntries: targetEntries
        )
        if !conflicts.isEmpty {
            throw MuonGitError.conflict("checkout would overwrite local changes: \(conflicts.joined(separator: ", "))")
        }
    }

    try writeIndex(gitDir: gitDir, index: indexFromMaterialized(targetEntries))
    let update = try applyWorkdirTree(workdir: workdir, currentIndex: currentIndex, targetEntries: targetEntries)
    try writeReference(gitDir: gitDir, name: "HEAD", oid: targetOID)

    let oldOID = previousHead ?? OID.zero
    try appendReflog(
        gitDir: gitDir,
        refName: "HEAD",
        oldOid: oldOID,
        newOid: targetOID,
        committer: defaultSignature(),
        message: "checkout: moving from \(currentDesc) to \(spec)"
    )

    return SwitchResult(
        previousHead: previousHead,
        headOID: targetOID,
        headRef: nil,
        updatedPaths: update.updatedPaths,
        removedPaths: update.removedPaths
    )
}

public func reset(
    gitDir: String,
    workdir: String?,
    spec: String,
    mode: ResetMode
) throws -> ResetResult {
    let targetOID = try resolveRevision(gitDir: gitDir, spec: spec)
    let previousHead = try currentHeadOID(gitDir: gitDir)
    let movedRef = try currentHeadTargetRef(gitDir: gitDir)
    let currentIndex = try readIndex(gitDir: gitDir)

    if let movedRef {
        try writeReference(gitDir: gitDir, name: movedRef, oid: targetOID)
    } else {
        try writeReference(gitDir: gitDir, name: "HEAD", oid: targetOID)
    }

    var update = WorkdirUpdate()
    if mode != .soft {
        let targetEntries = try materializeCommitTree(gitDir: gitDir, commitOID: targetOID)
        try writeIndex(gitDir: gitDir, index: indexFromMaterialized(targetEntries))

        if mode == .hard {
            guard let workdir else {
                throw MuonGitError.bareRepo
            }
            update = try applyWorkdirTree(workdir: workdir, currentIndex: currentIndex, targetEntries: targetEntries)
        }
    }

    let message = "reset: moving to \(spec)"
    let sig = defaultSignature()
    if let movedRef {
        try appendReflog(
            gitDir: gitDir,
            refName: movedRef,
            oldOid: previousHead,
            newOid: targetOID,
            committer: sig,
            message: message
        )
    }
    try appendReflog(
        gitDir: gitDir,
        refName: "HEAD",
        oldOid: previousHead,
        newOid: targetOID,
        committer: sig,
        message: message
    )

    return ResetResult(
        previousHead: previousHead,
        headOID: targetOID,
        movedRef: movedRef,
        updatedPaths: update.updatedPaths,
        removedPaths: update.removedPaths
    )
}

public func restore(
    gitDir: String,
    workdir: String?,
    paths: [String],
    options: RestoreOptions
) throws -> RestoreResult {
    let worktreeRequested = options.worktree || !options.staged
    let sourceSpec: String?
    if options.source != nil || options.staged {
        sourceSpec = options.source ?? "HEAD"
    } else {
        sourceSpec = nil
    }

    let sourceEntries: [String: MaterializedEntry]?
    if let sourceSpec {
        sourceEntries = try materializeRevisionTree(gitDir: gitDir, spec: sourceSpec)
    } else {
        sourceEntries = nil
    }

    let originalIndex = try readIndex(gitDir: gitDir)
    var index = originalIndex
    var result = RestoreResult()

    for path in paths where options.staged {
        guard let sourceEntries else {
            break
        }

        if let entry = sourceEntries[path] {
            index.add(indexEntryFromMaterialized(path: path, entry: entry))
            result.stagedPaths.append(path)
        } else if index.remove(path: path) {
            result.removedFromIndex.append(path)
        } else {
            throw MuonGitError.notFound("path '\(path)' not found in restore source")
        }
    }

    if options.staged {
        try writeIndex(gitDir: gitDir, index: index)
    }

    if worktreeRequested {
        guard let workdir else {
            throw MuonGitError.bareRepo
        }

        for path in paths {
            let targetPath = (workdir as NSString).appendingPathComponent(path)
            let knownPath = originalIndex.find(path: path) != nil || FileManager.default.fileExists(atPath: targetPath)

            if let sourceEntries, options.source != nil {
                try restorePathFromMaterialized(
                    workdir: workdir,
                    path: path,
                    entry: sourceEntries[path],
                    knownPath: knownPath,
                    result: &result
                )
                continue
            }

            try restorePathFromIndex(
                gitDir: gitDir,
                workdir: workdir,
                path: path,
                entry: index.find(path: path),
                knownPath: knownPath,
                result: &result
            )
        }
    }

    return result
}

public extension Repository {
    func checkoutIndex(options: CheckoutOptions) throws -> CheckoutResult {
        guard let workdir else {
            throw MuonGitError.bareRepo
        }
        return try MuonGit.checkoutIndex(gitDir: gitDir, workdir: workdir, options: options)
    }

    func checkoutPaths(_ paths: [String], options: CheckoutOptions) throws -> CheckoutResult {
        guard let workdir else {
            throw MuonGitError.bareRepo
        }
        return try MuonGit.checkoutPaths(gitDir: gitDir, workdir: workdir, paths: paths, options: options)
    }

    func switchBranch(name: String, options: SwitchOptions = SwitchOptions()) throws -> SwitchResult {
        guard let workdir else {
            throw MuonGitError.bareRepo
        }
        return try MuonGit.switchBranch(gitDir: gitDir, workdir: workdir, name: name, options: options)
    }

    func checkoutRevision(spec: String, options: SwitchOptions = SwitchOptions()) throws -> SwitchResult {
        guard let workdir else {
            throw MuonGitError.bareRepo
        }
        return try MuonGit.checkoutRevision(gitDir: gitDir, workdir: workdir, spec: spec, options: options)
    }

    func reset(spec: String, mode: ResetMode) throws -> ResetResult {
        try MuonGit.reset(gitDir: gitDir, workdir: workdir, spec: spec, mode: mode)
    }

    func restore(paths: [String], options: RestoreOptions = RestoreOptions()) throws -> RestoreResult {
        try MuonGit.restore(gitDir: gitDir, workdir: workdir, paths: paths, options: options)
    }
}

private func checkoutEntry(gitDir: String, workdir: String, entry: IndexEntry, options: CheckoutOptions, result: inout CheckoutResult) throws {
    let targetPath = (workdir as NSString).appendingPathComponent(entry.path)

    if !options.force && FileManager.default.fileExists(atPath: targetPath) {
        result.conflicts.append(entry.path)
        return
    }

    let parentDir = (targetPath as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parentDir, withIntermediateDirectories: true)

    let blob = try readBlob(gitDir: gitDir, oid: entry.oid)
    try blob.data.write(to: URL(fileURLWithPath: targetPath))
    try setMode(path: targetPath, mode: entry.mode)

    result.updated.append(entry.path)
}

private func currentHeadTargetRef(gitDir: String) throws -> String? {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    guard head.hasPrefix("ref: ") else {
        return nil
    }
    return String(head.dropFirst(5)).trimmingCharacters(in: .whitespacesAndNewlines)
}

private func currentHeadOID(gitDir: String) throws -> OID {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    if head.hasPrefix("ref: ") {
        do {
            return try resolveReference(gitDir: gitDir, name: "HEAD")
        } catch MuonGitError.notFound(_) {
            throw MuonGitError.unbornBranch
        }
    }
    return OID(hex: head.trimmingCharacters(in: .whitespacesAndNewlines))
}

private func describeHead(gitDir: String) throws -> String {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    if head.hasPrefix("ref: ") {
        let target = String(head.dropFirst(5)).trimmingCharacters(in: .whitespacesAndNewlines)
        return target.hasPrefix("refs/heads/")
            ? String(target.dropFirst("refs/heads/".count))
            : target
    }
    return shortOID(OID(hex: head.trimmingCharacters(in: .whitespacesAndNewlines)))
}

private func shortOID(_ oid: OID) -> String {
    String(oid.hex.prefix(7))
}

private func defaultSignature() -> Signature {
    Signature(name: "MuonGit", email: "muongit@example.invalid", time: 0, offset: 0)
}

private func materializeRevisionTree(gitDir: String, spec: String) throws -> [String: MaterializedEntry] {
    try materializeCommitTree(gitDir: gitDir, commitOID: resolveRevision(gitDir: gitDir, spec: spec))
}

private func materializeCommitTree(gitDir: String, commitOID: OID) throws -> [String: MaterializedEntry] {
    let commit = try revisionReadCommit(gitDir: gitDir, oid: commitOID)
    var entries: [String: MaterializedEntry] = [:]
    try collectTreeEntries(gitDir: gitDir, treeOID: commit.treeId, prefix: "", entries: &entries)
    return entries
}

private func collectTreeEntries(
    gitDir: String,
    treeOID: OID,
    prefix: String,
    entries: inout [String: MaterializedEntry]
) throws {
    let tree = try readObject(gitDir: gitDir, oid: treeOID).asTree()
    for entry in tree.entries {
        let path = prefix.isEmpty ? entry.name : "\(prefix)/\(entry.name)"
        if entry.mode == FileMode.tree.rawValue {
            try collectTreeEntries(gitDir: gitDir, treeOID: entry.oid, prefix: path, entries: &entries)
        } else {
            let blob = try readBlob(gitDir: gitDir, oid: entry.oid)
            entries[path] = MaterializedEntry(oid: entry.oid, mode: entry.mode, data: blob.data)
        }
    }
}

private func indexFromMaterialized(_ entries: [String: MaterializedEntry]) -> Index {
    var index = Index()
    for path in entries.keys.sorted() {
        guard let entry = entries[path] else {
            continue
        }
        index.add(indexEntryFromMaterialized(path: path, entry: entry))
    }
    return index
}

private func indexEntryFromMaterialized(path: String, entry: MaterializedEntry) -> IndexEntry {
    IndexEntry(
        mode: entry.mode,
        fileSize: UInt32(entry.data.count),
        oid: entry.oid,
        flags: UInt16(min(path.utf8.count, 0x0FFF)),
        path: path
    )
}

private func collectSwitchConflicts(
    gitDir: String,
    workdir: String,
    currentIndex: Index,
    targetEntries: [String: MaterializedEntry]
) throws -> [String] {
    var conflicts = Set<String>()

    for path in try stagedChangePaths(gitDir: gitDir, currentIndex: currentIndex) {
        conflicts.insert(path)
    }

    for path in targetEntries.keys.sorted() {
        if let current = currentIndex.find(path: path) {
            if try !workdirMatchesEntry(workdir: workdir, entry: current) {
                conflicts.insert(path)
            }
        } else if FileManager.default.fileExists(atPath: (workdir as NSString).appendingPathComponent(path)) {
            conflicts.insert(path)
        }
    }

    for entry in currentIndex.entries where targetEntries[entry.path] == nil {
        if try !workdirMatchesEntry(workdir: workdir, entry: entry) {
            conflicts.insert(entry.path)
        }
    }

    return conflicts.sorted()
}

private func stagedChangePaths(gitDir: String, currentIndex: Index) throws -> [String] {
    let currentHead: OID?
    do {
        currentHead = try currentHeadOID(gitDir: gitDir)
    } catch MuonGitError.unbornBranch {
        currentHead = nil
    }

    let headEntries = try currentHead.map { try materializeCommitTree(gitDir: gitDir, commitOID: $0) } ?? [:]
    let currentPaths = Set(currentIndex.entries.map(\.path))
    let headPaths = Set(headEntries.keys)
    var changes = Set<String>()

    for entry in currentIndex.entries {
        if let headEntry = headEntries[entry.path] {
            if headEntry.oid != entry.oid || headEntry.mode != entry.mode {
                changes.insert(entry.path)
            }
        } else {
            changes.insert(entry.path)
        }
    }

    for path in headPaths.subtracting(currentPaths) {
        changes.insert(path)
    }

    return changes.sorted()
}

private func workdirMatchesEntry(workdir: String, entry: IndexEntry) throws -> Bool {
    let targetPath = (workdir as NSString).appendingPathComponent(entry.path)
    guard FileManager.default.fileExists(atPath: targetPath) else {
        return false
    }

    let attributes = try FileManager.default.attributesOfItem(atPath: targetPath)
    if let type = attributes[.type] as? FileAttributeType, type != .typeRegular {
        return false
    }

    let content = try Data(contentsOf: URL(fileURLWithPath: targetPath))
    if content.count != Int(entry.fileSize) {
        return false
    }
    if OID.hash(type: .blob, data: Array(content)) != entry.oid {
        return false
    }

    if let perms = attributes[.posixPermissions] as? NSNumber {
        let isExecutable = (perms.intValue & 0o111) != 0
        let expected = (Int(entry.mode) & 0o111) != 0
        if isExecutable != expected {
            return false
        }
    }

    return true
}

private func applyWorkdirTree(
    workdir: String,
    currentIndex: Index,
    targetEntries: [String: MaterializedEntry]
) throws -> WorkdirUpdate {
    var update = WorkdirUpdate()
    let currentPaths = Set(currentIndex.entries.map(\.path))
    let targetPaths = Set(targetEntries.keys)

    for path in currentPaths.subtracting(targetPaths).sorted() {
        let filePath = (workdir as NSString).appendingPathComponent(path)
        if FileManager.default.fileExists(atPath: filePath) {
            try removeWorkdirPath(root: workdir, path: filePath)
            update.removedPaths.append(path)
        }
    }

    for path in targetEntries.keys.sorted() {
        guard let entry = targetEntries[path] else {
            continue
        }
        try writeMaterializedToWorkdir(workdir: workdir, path: path, entry: entry)
        update.updatedPaths.append(path)
    }

    return update
}

private func restorePathFromMaterialized(
    workdir: String,
    path: String,
    entry: MaterializedEntry?,
    knownPath: Bool,
    result: inout RestoreResult
) throws {
    if let entry {
        try writeMaterializedToWorkdir(workdir: workdir, path: path, entry: entry)
        result.restoredPaths.append(path)
        return
    }

    let target = (workdir as NSString).appendingPathComponent(path)
    if FileManager.default.fileExists(atPath: target) {
        try removeWorkdirPath(root: workdir, path: target)
        result.removedFromWorkdir.append(path)
    } else if !knownPath {
        throw MuonGitError.notFound("path '\(path)' not found")
    }
}

private func restorePathFromIndex(
    gitDir: String,
    workdir: String,
    path: String,
    entry: IndexEntry?,
    knownPath: Bool,
    result: inout RestoreResult
) throws {
    if let entry {
        try writeIndexEntryToWorkdir(gitDir: gitDir, workdir: workdir, entry: entry)
        result.restoredPaths.append(path)
        return
    }

    let target = (workdir as NSString).appendingPathComponent(path)
    if FileManager.default.fileExists(atPath: target) {
        try removeWorkdirPath(root: workdir, path: target)
        result.removedFromWorkdir.append(path)
    } else if !knownPath {
        throw MuonGitError.notFound("path '\(path)' not found")
    }
}

private func writeMaterializedToWorkdir(workdir: String, path: String, entry: MaterializedEntry) throws {
    let target = (workdir as NSString).appendingPathComponent(path)
    let parent = (target as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parent, withIntermediateDirectories: true)
    try entry.data.write(to: URL(fileURLWithPath: target))
    try setMode(path: target, mode: entry.mode)
}

private func writeIndexEntryToWorkdir(gitDir: String, workdir: String, entry: IndexEntry) throws {
    let target = (workdir as NSString).appendingPathComponent(entry.path)
    let parent = (target as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parent, withIntermediateDirectories: true)
    let blob = try readBlob(gitDir: gitDir, oid: entry.oid)
    try blob.data.write(to: URL(fileURLWithPath: target))
    try setMode(path: target, mode: entry.mode)
}

private func setMode(path: String, mode: UInt32) throws {
    let perms = (mode & 0o111) != 0 ? 0o755 : 0o644
    try FileManager.default.setAttributes([.posixPermissions: perms], ofItemAtPath: path)
}

private func removeWorkdirPath(root: String, path: String) throws {
    let fm = FileManager.default
    if fm.fileExists(atPath: path) {
        try fm.removeItem(atPath: path)
    }

    var current = (path as NSString).deletingLastPathComponent
    while current != root {
        let contents = try fm.contentsOfDirectory(atPath: current)
        if !contents.isEmpty {
            break
        }
        try fm.removeItem(atPath: current)
        let next = (current as NSString).deletingLastPathComponent
        if next == current {
            break
        }
        current = next
    }
}
