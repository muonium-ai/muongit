/// MuonGit - Revert support
/// Parity: libgit2 src/libgit2/revert.c
import Foundation

/// Options for revert
public struct RevertOptions: Sendable {
    /// For merge commits, which parent to use (1-based, default 1)
    public var mainline: Int
    public init(mainline: Int = 1) { self.mainline = mainline }
}

/// Result of a revert operation
public struct RevertResult: Sendable {
    public let hasConflicts: Bool
    /// Merged file contents: (path, content, conflicted)
    public let files: [(String, String, Bool)]
    public let revertedCommit: OID
}

/// Revert a commit against HEAD.
///
/// Three-way merge with swapped args: merge(commit_tree, head_tree, parent_tree)
public func revert(
    gitDir: String,
    commitOid: OID,
    options: RevertOptions = RevertOptions()
) throws -> RevertResult {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: commitOid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("not a commit")
    }
    let commit = try parseCommit(oid: commitOid, data: data)

    guard !commit.parentIds.isEmpty else {
        throw MuonGitError.invalidObject("cannot revert a root commit")
    }
    let parentIdx = max(options.mainline - 1, 0)
    guard parentIdx < commit.parentIds.count else {
        throw MuonGitError.invalidObject("mainline parent not found")
    }
    let parentOid = commit.parentIds[parentIdx]

    // Revert swaps base and theirs compared to cherry-pick
    let commitTree = try loadCommitTreeDirect(gitDir: gitDir, commit: commit)
    let headOid = try resolveReference(gitDir: gitDir, name: "HEAD")
    let headTree = try loadCommitTree(gitDir: gitDir, commitOid: headOid)
    let parentTree = try loadCommitTree(gitDir: gitDir, commitOid: parentOid)

    let result = try mergeTreesContent(
        gitDir: gitDir,
        base: commitTree,
        ours: headTree,
        theirs: parentTree
    )

    // Write state files
    try commitOid.hex.write(toFile: gitDir + "/REVERT_HEAD", atomically: true, encoding: .utf8)
    let revertMsg = "Revert \"\(commit.message.trimmingCharacters(in: .whitespacesAndNewlines))\"\n\nThis reverts commit \(commitOid.hex).\n"
    try revertMsg.write(toFile: gitDir + "/MERGE_MSG", atomically: true, encoding: .utf8)

    return RevertResult(
        hasConflicts: result.hasConflicts,
        files: result.files,
        revertedCommit: commitOid
    )
}

/// Clean up revert state files
public func revertCleanup(gitDir: String) {
    let fm = FileManager.default
    try? fm.removeItem(atPath: gitDir + "/REVERT_HEAD")
    try? fm.removeItem(atPath: gitDir + "/MERGE_MSG")
}
