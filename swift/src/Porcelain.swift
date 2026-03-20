/// MuonGit - High-level staging and commit workflows
import Foundation

public struct AddOptions: Sendable, Equatable {
    public var includeIgnored: Bool

    public init(includeIgnored: Bool = false) {
        self.includeIgnored = includeIgnored
    }
}

public struct AddResult: Sendable, Equatable {
    public var stagedPaths: [String]
    public var removedPaths: [String]

    public init(stagedPaths: [String] = [], removedPaths: [String] = []) {
        self.stagedPaths = stagedPaths
        self.removedPaths = removedPaths
    }
}

public struct RemoveResult: Sendable, Equatable {
    public var removedFromIndex: [String]
    public var removedFromWorkdir: [String]

    public init(removedFromIndex: [String] = [], removedFromWorkdir: [String] = []) {
        self.removedFromIndex = removedFromIndex
        self.removedFromWorkdir = removedFromWorkdir
    }
}

public struct UnstageResult: Sendable, Equatable {
    public var restoredPaths: [String]
    public var removedPaths: [String]

    public init(restoredPaths: [String] = [], removedPaths: [String] = []) {
        self.restoredPaths = restoredPaths
        self.removedPaths = removedPaths
    }
}

public struct CommitOptions: Sendable {
    public var author: Signature?
    public var committer: Signature?

    public init(author: Signature? = nil, committer: Signature? = nil) {
        self.author = author
        self.committer = committer
    }
}

public struct CommitResult: Sendable, Equatable {
    public let oid: OID
    public let treeID: OID
    public let parentIDs: [OID]
    public let reference: String
    public let summary: String

    public init(oid: OID, treeID: OID, parentIDs: [OID], reference: String, summary: String) {
        self.oid = oid
        self.treeID = treeID
        self.parentIDs = parentIDs
        self.reference = reference
        self.summary = summary
    }
}

public func addPaths(
    gitDir: String,
    workdir: String,
    patterns: [String],
    options: AddOptions = AddOptions()
) throws -> AddResult {
    var index = try readIndex(gitDir: gitDir)
    var candidates = try collectWorkdirPaths(gitDir: gitDir, workdir: workdir, includeIgnored: options.includeIgnored)
    index.entries.forEach { candidates.insert($0.path) }
    let matched = try matchPatterns(candidates: candidates, patterns: patterns)
    var result = AddResult()

    for path in matched {
        let fullPath = (workdir as NSString).appendingPathComponent(path)
        guard FileManager.default.fileExists(atPath: fullPath) else {
            if index.remove(path: path) {
                result.removedPaths.append(path)
            }
            continue
        }

        try stagePath(gitDir: gitDir, workdir: workdir, index: &index, path: path)
        result.stagedPaths.append(path)
    }

    try writeIndex(gitDir: gitDir, index: index)
    return result
}

public func removePaths(
    gitDir: String,
    workdir: String,
    patterns: [String]
) throws -> RemoveResult {
    var index = try readIndex(gitDir: gitDir)
    var candidates = try collectWorkdirPaths(gitDir: gitDir, workdir: workdir, includeIgnored: true)
    index.entries.forEach { candidates.insert($0.path) }
    let matched = try matchPatterns(candidates: candidates, patterns: patterns)
    var result = RemoveResult()

    for path in matched {
        if index.remove(path: path) {
            result.removedFromIndex.append(path)
        }

        let fullPath = (workdir as NSString).appendingPathComponent(path)
        if FileManager.default.fileExists(atPath: fullPath) {
            try removeWorkdirPath(workdir: workdir, target: fullPath)
            result.removedFromWorkdir.append(path)
        }
    }

    try writeIndex(gitDir: gitDir, index: index)
    return result
}

public func unstagePaths(gitDir: String, patterns: [String]) throws -> UnstageResult {
    var index = try readIndex(gitDir: gitDir)
    let headEntries = try readHeadIndexEntries(gitDir: gitDir)
    var candidates = Set(index.entries.map(\.path))
    headEntries.keys.forEach { candidates.insert($0) }
    let matched = try matchPatterns(candidates: candidates, patterns: patterns)
    var result = UnstageResult()

    for path in matched {
        if let entry = headEntries[path] {
            index.add(entry)
            result.restoredPaths.append(path)
        } else if index.remove(path: path) {
            result.removedPaths.append(path)
        }
    }

    try writeIndex(gitDir: gitDir, index: index)
    return result
}

public func createCommit(
    gitDir: String,
    message: String,
    options: CommitOptions = CommitOptions()
) throws -> CommitResult {
    guard let headRef = try currentHeadRef(gitDir: gitDir) else {
        throw MuonGitError.invalidSpec("cannot commit on detached HEAD")
    }

    let parentOID: OID?
    do {
        parentOID = try resolveReference(gitDir: gitDir, name: "HEAD")
    } catch MuonGitError.notFound {
        parentOID = nil
    }

    let index = try readIndex(gitDir: gitDir)
    let treeID = try writeTreeFromIndex(gitDir: gitDir, index: index)
    let author = options.author ?? defaultSignature()
    let committer = options.committer ?? author
    let normalizedMessage = normalizeCommitMessage(message)
    let summary = commitSummary(normalizedMessage)
    let parentIDs = parentOID.map { [$0] } ?? []
    let data = serializeCommit(
        treeId: treeID,
        parentIds: parentIDs,
        author: author,
        committer: committer,
        message: normalizedMessage
    )
    let commitOID = try writeLooseObject(gitDir: gitDir, type: .commit, data: data)
    try writeReference(gitDir: gitDir, name: headRef, oid: commitOID)

    let oldOID = parentOID ?? OID.zero
    let reflogMessage = oldOID.isZero ? "commit (initial): \(summary)" : "commit: \(summary)"
    try appendReflog(gitDir: gitDir, refName: headRef, oldOid: oldOID, newOid: commitOID, committer: committer, message: reflogMessage)
    try appendReflog(gitDir: gitDir, refName: "HEAD", oldOid: oldOID, newOid: commitOID, committer: committer, message: reflogMessage)

    return CommitResult(
        oid: commitOID,
        treeID: treeID,
        parentIDs: parentIDs,
        reference: headRef,
        summary: summary
    )
}

public extension Repository {
    func add(paths: [String], options: AddOptions = AddOptions()) throws -> AddResult {
        guard let workdir else { throw MuonGitError.bareRepo }
        return try addPaths(gitDir: gitDir, workdir: workdir, patterns: paths, options: options)
    }

    func remove(paths: [String]) throws -> RemoveResult {
        guard let workdir else { throw MuonGitError.bareRepo }
        return try removePaths(gitDir: gitDir, workdir: workdir, patterns: paths)
    }

    func unstage(paths: [String]) throws -> UnstageResult {
        try unstagePaths(gitDir: gitDir, patterns: paths)
    }

    func commit(message: String, options: CommitOptions = CommitOptions()) throws -> CommitResult {
        try createCommit(gitDir: gitDir, message: message, options: options)
    }
}

private func matchPatterns(candidates: Set<String>, patterns: [String]) throws -> [String] {
    let ordered = candidates.sorted()
    if ordered.isEmpty {
        throw MuonGitError.notFound("no paths available")
    }
    if patterns.isEmpty {
        return ordered
    }

    let pathspec = Pathspec(patterns: patterns)
    let result = pathspec.matchPaths(ordered, flags: PathspecFlags(findFailures: true))
    if !result.failures.isEmpty {
        throw MuonGitError.notFound("pathspec did not match: \(result.failures.joined(separator: ", "))")
    }
    return result.matches
}

private func collectWorkdirPaths(
    gitDir: String,
    workdir: String,
    includeIgnored: Bool
) throws -> Set<String> {
    var paths = Set<String>()
    let ignore = Ignore.load(gitDir: gitDir, workdir: workdir)
    try collectWorkdirPathsRecursive(
        dir: workdir,
        workdir: workdir,
        gitDir: gitDir,
        includeIgnored: includeIgnored,
        ignore: ignore,
        paths: &paths
    )
    return paths
}

private func collectWorkdirPathsRecursive(
    dir: String,
    workdir: String,
    gitDir: String,
    includeIgnored: Bool,
    ignore: Ignore,
    paths: inout Set<String>
) throws {
    var scopedIgnore = ignore
    try scopedIgnore.loadForPath(workdir: workdir, relDir: relativePath(dir, from: workdir))
    let children = try FileManager.default.contentsOfDirectory(atPath: dir).sorted()

    for child in children {
        let fullPath = (dir as NSString).appendingPathComponent(child)
        if fullPath == gitDir || child == ".git" {
            continue
        }

        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: fullPath, isDirectory: &isDir) else {
            continue
        }

        let relPath = try relativePath(fullPath, from: workdir)
        if isDir.boolValue {
            if !includeIgnored && scopedIgnore.isIgnored(relPath, isDir: true) {
                continue
            }
            try collectWorkdirPathsRecursive(
                dir: fullPath,
                workdir: workdir,
                gitDir: gitDir,
                includeIgnored: includeIgnored,
                ignore: scopedIgnore,
                paths: &paths
            )
        } else {
            if !includeIgnored && scopedIgnore.isIgnored(relPath, isDir: false) {
                continue
            }
            paths.insert(relPath)
        }
    }
}

private func relativePath(_ path: String, from workdir: String) throws -> String {
    if path == workdir {
        return ""
    }

    let prefix = workdir.hasSuffix("/") ? workdir : workdir + "/"
    guard path.hasPrefix(prefix) else {
        throw MuonGitError.invalid("path is outside repository workdir")
    }
    return String(path.dropFirst(prefix.count))
}

private func stagePath(
    gitDir: String,
    workdir: String,
    index: inout Index,
    path: String
) throws {
    let fullPath = (workdir as NSString).appendingPathComponent(path)
    let raw = try Data(contentsOf: URL(fileURLWithPath: fullPath))
    let filtered = FilterList.load(gitDir: gitDir, workdir: workdir, path: path, mode: .toOdb).apply(raw)
    let attributes = try FileManager.default.attributesOfItem(atPath: fullPath)
    let size = UInt32(truncatingIfNeeded: (attributes[.size] as? NSNumber)?.uint64Value ?? UInt64(raw.count))
    let perms = (attributes[.posixPermissions] as? NSNumber)?.uint16Value ?? 0
    let mode = (perms & 0o111) != 0 ? FileMode.blobExe.rawValue : FileMode.blob.rawValue
    let oid = try writeLooseObject(gitDir: gitDir, type: .blob, data: filtered)

    index.add(
        IndexEntry(
            mode: mode,
            fileSize: size,
            oid: oid,
            flags: UInt16(min(path.utf8.count, 0x0FFF)),
            path: path
        )
    )
}

private func readHeadIndexEntries(gitDir: String) throws -> [String: IndexEntry] {
    let headOID: OID
    do {
        headOID = try resolveReference(gitDir: gitDir, name: "HEAD")
    } catch MuonGitError.notFound {
        return [:]
    }

    let commit = try readObject(gitDir: gitDir, oid: headOID).asCommit()
    var entries: [String: IndexEntry] = [:]
    try collectHeadTreeEntries(gitDir: gitDir, treeOID: commit.treeId, prefix: "", entries: &entries)
    return entries
}

private func collectHeadTreeEntries(
    gitDir: String,
    treeOID: OID,
    prefix: String,
    entries: inout [String: IndexEntry]
) throws {
    let tree = try readObject(gitDir: gitDir, oid: treeOID).asTree()
    for entry in tree.entries {
        let path = prefix.isEmpty ? entry.name : "\(prefix)/\(entry.name)"
        if entry.mode == FileMode.tree.rawValue {
            try collectHeadTreeEntries(gitDir: gitDir, treeOID: entry.oid, prefix: path, entries: &entries)
        } else {
            let blob = try readBlob(gitDir: gitDir, oid: entry.oid)
            entries[path] = IndexEntry(
                mode: entry.mode,
                fileSize: UInt32(blob.data.count),
                oid: entry.oid,
                flags: UInt16(min(path.utf8.count, 0x0FFF)),
                path: path
            )
        }
    }
}

final class TreeNode {
    var files: [TreeEntry] = []
    var children: [String: TreeNode] = [:]
}

func writeTreeFromIndex(gitDir: String, index: Index) throws -> OID {
    let root = TreeNode()
    for entry in index.entries {
        try insertTreeEntry(root, entry: entry)
    }
    return try writeTreeNode(gitDir: gitDir, node: root)
}

private func insertTreeEntry(_ node: TreeNode, entry: IndexEntry) throws {
    let parts = entry.path.split(separator: "/").map(String.init)
    guard !parts.isEmpty else {
        throw MuonGitError.invalid("empty index path")
    }
    try insertTreeEntryParts(node, entry: entry, parts: parts, depth: 0)
}

private func insertTreeEntryParts(
    _ node: TreeNode,
    entry: IndexEntry,
    parts: [String],
    depth: Int
) throws {
    let part = parts[depth]
    if depth == parts.count - 1 {
        node.files.append(TreeEntry(mode: entry.mode, name: part, oid: entry.oid))
        return
    }

    let child = node.children[part] ?? TreeNode()
    node.children[part] = child
    try insertTreeEntryParts(child, entry: entry, parts: parts, depth: depth + 1)
}

private func writeTreeNode(gitDir: String, node: TreeNode) throws -> OID {
    var entries = node.files
    for name in node.children.keys.sorted() {
        guard let child = node.children[name] else { continue }
        let childOID = try writeTreeNode(gitDir: gitDir, node: child)
        entries.append(TreeEntry(mode: FileMode.tree.rawValue, name: name, oid: childOID))
    }
    return try writeLooseObject(gitDir: gitDir, type: .tree, data: serializeTree(entries: entries))
}

private func currentHeadRef(gitDir: String) throws -> String? {
    let head = try readReference(gitDir: gitDir, name: "HEAD")
    guard head.hasPrefix("ref: ") else {
        return nil
    }
    return String(head.dropFirst(5))
}

private func normalizeCommitMessage(_ message: String) -> String {
    message.hasSuffix("\n") ? message : message + "\n"
}

private func commitSummary(_ message: String) -> String {
    message.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? ""
}

private func defaultSignature() -> Signature {
    Signature(name: "MuonGit", email: "muongit@example.invalid")
}

private func removeWorkdirPath(workdir: String, target: String) throws {
    try FileManager.default.removeItem(atPath: target)
    pruneEmptyParents(workdir: workdir, current: (target as NSString).deletingLastPathComponent)
}

private func pruneEmptyParents(workdir: String, current: String) {
    var path = current
    while !path.isEmpty && path != workdir {
        let contents = (try? FileManager.default.contentsOfDirectory(atPath: path)) ?? []
        if !contents.isEmpty {
            break
        }
        try? FileManager.default.removeItem(atPath: path)
        path = (path as NSString).deletingLastPathComponent
    }
}
