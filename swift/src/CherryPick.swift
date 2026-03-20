/// MuonGit - Cherry-pick support
/// Parity: libgit2 src/libgit2/cherrypick.c
import Foundation

/// Options for cherry-pick
public struct CherryPickOptions: Sendable {
    /// For merge commits, which parent to diff against (1-based, default 1)
    public var mainline: Int
    public init(mainline: Int = 1) { self.mainline = mainline }
}

/// Result of a cherry-pick operation
public struct CherryPickResult: Sendable {
    public let hasConflicts: Bool
    /// Merged file contents: (path, content, conflicted)
    public let files: [(String, String, Bool)]
    public let cherryPickedCommit: OID
}

/// Result of merging tree contents (shared with Revert and Rebase)
public struct TreeMergeResult: Sendable {
    public let hasConflicts: Bool
    public let files: [(String, String, Bool)]
}

/// Cherry-pick a commit onto HEAD.
///
/// Three-way merge: merge(parent_tree, head_tree, commit_tree)
public func cherryPick(
    gitDir: String,
    commitOid: OID,
    options: CherryPickOptions = CherryPickOptions()
) throws -> CherryPickResult {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: commitOid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("not a commit")
    }
    let commit = try parseCommit(oid: commitOid, data: data)

    guard !commit.parentIds.isEmpty else {
        throw MuonGitError.invalidObject("cannot cherry-pick a root commit")
    }
    let parentIdx = max(options.mainline - 1, 0)
    guard parentIdx < commit.parentIds.count else {
        throw MuonGitError.invalidObject("mainline parent not found")
    }
    let parentOid = commit.parentIds[parentIdx]

    let parentTree = try loadCommitTree(gitDir: gitDir, commitOid: parentOid)
    let commitTree = try loadCommitTreeDirect(gitDir: gitDir, commit: commit)
    let headOid = try resolveReference(gitDir: gitDir, name: "HEAD")
    let headTree = try loadCommitTree(gitDir: gitDir, commitOid: headOid)

    let result = try mergeTreesContent(
        gitDir: gitDir,
        base: parentTree,
        ours: headTree,
        theirs: commitTree
    )

    // Write state files
    let fm = FileManager.default
    try commitOid.hex.write(toFile: gitDir + "/CHERRY_PICK_HEAD", atomically: true, encoding: .utf8)
    try commit.message.write(toFile: gitDir + "/MERGE_MSG", atomically: true, encoding: .utf8)

    return CherryPickResult(
        hasConflicts: result.hasConflicts,
        files: result.files,
        cherryPickedCommit: commitOid
    )
}

/// Clean up cherry-pick state files
public func cherryPickCleanup(gitDir: String) {
    let fm = FileManager.default
    try? fm.removeItem(atPath: gitDir + "/CHERRY_PICK_HEAD")
    try? fm.removeItem(atPath: gitDir + "/MERGE_MSG")
}

// MARK: - Shared helpers

func loadCommitTree(gitDir: String, commitOid: OID) throws -> [TreeEntry] {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: commitOid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("expected commit")
    }
    let commit = try parseCommit(oid: commitOid, data: data)
    return try loadTreeEntries(gitDir: gitDir, treeOid: commit.treeId)
}

func loadCommitTreeDirect(gitDir: String, commit: Commit) throws -> [TreeEntry] {
    return try loadTreeEntries(gitDir: gitDir, treeOid: commit.treeId)
}

func loadTreeEntries(gitDir: String, treeOid: OID) throws -> [TreeEntry] {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: treeOid)
    guard objType == .tree else {
        throw MuonGitError.invalidObject("expected tree")
    }
    return try parseTree(oid: treeOid, data: data).entries
}

func readBlobText(gitDir: String, oid: OID) -> String {
    if oid.isZero { return "" }
    guard let (objType, data) = try? readLooseObject(gitDir: gitDir, oid: oid),
          objType == .blob else { return "" }
    return String(data: data, encoding: .utf8) ?? ""
}

/// Merge two trees against a base, producing per-file merge results.
public func mergeTreesContent(
    gitDir: String,
    base: [TreeEntry],
    ours: [TreeEntry],
    theirs: [TreeEntry]
) throws -> TreeMergeResult {
    // Index by name
    var allPaths: [String: (OID?, OID?, OID?)] = [:]
    for e in base {
        allPaths[e.name, default: (nil, nil, nil)].0 = e.oid
    }
    for e in ours {
        allPaths[e.name, default: (nil, nil, nil)].1 = e.oid
    }
    for e in theirs {
        allPaths[e.name, default: (nil, nil, nil)].2 = e.oid
    }

    var files: [(String, String, Bool)] = []
    var hasConflicts = false
    let zero = OID.zero

    for path in allPaths.keys.sorted() {
        let (baseOid, oursOid, theirsOid) = allPaths[path]!
        let b = baseOid ?? zero
        let o = oursOid ?? zero
        let t = theirsOid ?? zero

        if o == t {
            let content = readBlobText(gitDir: gitDir, oid: o)
            files.append((path, content, false))
            continue
        }
        if o == b {
            if t.isZero { continue }
            let content = readBlobText(gitDir: gitDir, oid: t)
            files.append((path, content, false))
            continue
        }
        if t == b {
            if o.isZero { continue }
            let content = readBlobText(gitDir: gitDir, oid: o)
            files.append((path, content, false))
            continue
        }

        // Both sides changed — content-level merge
        let baseText = readBlobText(gitDir: gitDir, oid: b)
        let oursText = readBlobText(gitDir: gitDir, oid: o)
        let theirsText = readBlobText(gitDir: gitDir, oid: t)

        let mergeResult = merge3(base: baseText, ours: oursText, theirs: theirsText)
        if mergeResult.hasConflicts {
            hasConflicts = true
            files.append((path, mergeResult.toStringWithMarkers(), true))
        } else {
            files.append((path, mergeResult.toCleanString() ?? "", false))
        }
    }

    return TreeMergeResult(hasConflicts: hasConflicts, files: files)
}
