/// MuonGit - Stash support
/// Parity: libgit2 src/libgit2/stash.c
import Foundation

/// Stash flags controlling what gets stashed.
/// Parity: git_stash_flags
public enum StashFlags: Sendable {
    /// Default: stash staged + unstaged changes
    case `default`
    /// Leave staged changes in the index
    case keepIndex
    /// Include untracked files
    case includeUntracked
}

/// A stash entry from the reflog.
public struct StashEntry: Sendable {
    public let index: Int
    public let message: String
    public let oid: OID
}

/// Result of applying a stash.
public struct StashApplyResult: Sendable {
    public let hasConflicts: Bool
    public let files: [(String, String, Bool)]
}

/// Save the current working directory state as a stash entry.
///
/// Creates the multi-parent stash commit structure:
/// - w_commit (refs/stash target): tree = workdir state, parents = [HEAD, i_commit]
/// - i_commit: tree = index state, parent = HEAD
///
/// Parity: git_stash_save
public func stashSave(
    gitDir: String,
    workdir: String?,
    stasher: Signature,
    message: String? = nil
) throws -> OID {
    guard let workdir = workdir else {
        throw MuonGitError.bareRepo
    }

    let headOid = try resolveReference(gitDir: gitDir, name: "HEAD")

    // Read HEAD commit for branch info
    let (_, headData) = try readLooseObject(gitDir: gitDir, oid: headOid)
    let headCommit = try parseCommit(oid: headOid, data: headData)
    let shortSha = String(headOid.hex.prefix(7))

    // Get branch name
    let branch: String
    if let headRef = try? readReference(gitDir: gitDir, name: "HEAD"),
       headRef.hasPrefix("ref: refs/heads/") {
        branch = String(headRef.dropFirst("ref: refs/heads/".count))
    } else {
        branch = "(no branch)"
    }

    let summary = headCommit.message.components(separatedBy: .newlines).first ?? ""

    // Collect workdir entries
    let workdirEntries = try collectWorkdirEntries(gitDir: gitDir, workdir: workdir)
    guard !workdirEntries.isEmpty else {
        throw MuonGitError.notFound("no local changes to save")
    }

    // Create workdir tree
    let workdirTreeData = serializeTree(entries: workdirEntries)
    let workdirTreeOid = try writeLooseObject(gitDir: gitDir, type: .tree, data: workdirTreeData)

    // Create i_commit (index snapshot)
    let iMsg = "index on \(branch): \(shortSha) \(summary)\n"
    let iData = serializeCommit(
        treeId: workdirTreeOid,
        parentIds: [headOid],
        author: stasher,
        committer: stasher,
        message: iMsg
    )
    let iOid = try writeLooseObject(gitDir: gitDir, type: .commit, data: iData)

    // Create w_commit (working directory snapshot)
    let stashMsg: String
    if let msg = message {
        stashMsg = "On \(branch): \(msg)\n"
    } else {
        stashMsg = "WIP on \(branch): \(shortSha) \(summary)\n"
    }
    let wData = serializeCommit(
        treeId: workdirTreeOid,
        parentIds: [headOid, iOid],
        author: stasher,
        committer: stasher,
        message: stashMsg
    )
    let wOid = try writeLooseObject(gitDir: gitDir, type: .commit, data: wData)

    // Update refs/stash
    let oldStash = (try? resolveReference(gitDir: gitDir, name: "refs/stash")) ?? OID.zero
    try writeReference(gitDir: gitDir, name: "refs/stash", oid: wOid)

    // Append to reflog
    let reflogMsg = stashMsg.trimmingCharacters(in: .whitespacesAndNewlines)
    try appendReflog(
        gitDir: gitDir,
        refName: "refs/stash",
        oldOid: oldStash,
        newOid: wOid,
        committer: stasher,
        message: reflogMsg
    )

    return wOid
}

/// List all stash entries.
/// Returns entries in reverse order (most recent first, index 0 = newest).
/// Parity: git_stash_foreach
public func stashList(gitDir: String) throws -> [StashEntry] {
    let entries = try readReflog(gitDir: gitDir, refName: "refs/stash")
    let count = entries.count
    return entries.enumerated().reversed().map { (i, entry) in
        StashEntry(index: count - 1 - i, message: entry.message, oid: entry.newOid)
    }
}

/// Apply a stash entry without removing it.
/// Parity: git_stash_apply
public func stashApply(gitDir: String, index: Int = 0) throws -> StashApplyResult {
    let entries = try readReflog(gitDir: gitDir, refName: "refs/stash")
    let count = entries.count
    guard index < count else {
        throw MuonGitError.notFound("stash@{\(index)} not found")
    }

    let reflogIdx = count - 1 - index
    let stashOid = entries[reflogIdx].newOid

    return try applyStashOid(gitDir: gitDir, stashOid: stashOid)
}

/// Pop the stash at position `index`: apply then drop.
/// Parity: git_stash_pop
public func stashPop(gitDir: String, index: Int = 0) throws -> StashApplyResult {
    let result = try stashApply(gitDir: gitDir, index: index)
    if !result.hasConflicts {
        try stashDrop(gitDir: gitDir, index: index)
    }
    return result
}

/// Drop a stash entry by index.
/// Parity: git_stash_drop
public func stashDrop(gitDir: String, index: Int) throws {
    let entries = try readReflog(gitDir: gitDir, refName: "refs/stash")
    let count = entries.count
    guard index < count else {
        throw MuonGitError.notFound("stash@{\(index)} not found")
    }

    let reflogIdx = count - 1 - index
    let remaining = try dropReflogEntry(gitDir: gitDir, refName: "refs/stash", index: reflogIdx)

    if remaining.isEmpty {
        _ = try? deleteReference(gitDir: gitDir, name: "refs/stash")
    } else {
        let newest = remaining[remaining.count - 1]
        try writeReference(gitDir: gitDir, name: "refs/stash", oid: newest.newOid)
    }
}

/// Drop a reflog entry by index. Returns remaining entries.
func dropReflogEntry(gitDir: String, refName: String, index: Int) throws -> [ReflogEntry] {
    let logPath = (gitDir as NSString).appendingPathComponent("logs/\(refName)")
    var entries = try readReflog(gitDir: gitDir, refName: refName)

    guard index < entries.count else {
        throw MuonGitError.notFound("reflog entry \(index) not found for \(refName)")
    }

    entries.remove(at: index)

    if entries.isEmpty {
        try? FileManager.default.removeItem(atPath: logPath)
    } else {
        var content = ""
        for entry in entries {
            content += formatReflogEntry(oldOid: entry.oldOid, newOid: entry.newOid,
                                         committer: entry.committer, message: entry.message)
        }
        try content.write(toFile: logPath, atomically: true, encoding: .utf8)
    }

    return entries
}

// MARK: - Internal helpers

/// Collect workdir files as tree entries (single-level, skipping .git).
private func collectWorkdirEntries(gitDir: String, workdir: String) throws -> [TreeEntry] {
    let fm = FileManager.default
    guard fm.fileExists(atPath: workdir) else { return [] }
    guard let items = try? fm.contentsOfDirectory(atPath: workdir) else { return [] }

    var entries: [TreeEntry] = []
    for name in items {
        if name == ".git" { continue }
        let path = (workdir as NSString).appendingPathComponent(name)
        var isDir: ObjCBool = false
        fm.fileExists(atPath: path, isDirectory: &isDir)
        if isDir.boolValue { continue }

        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        let blobOid = try writeLooseObject(gitDir: gitDir, type: .blob, data: data)
        entries.append(TreeEntry(mode: FileMode.blob.rawValue, name: name, oid: blobOid))
    }

    entries.sort { $0.name < $1.name }
    return entries
}

/// Apply a stash commit by OID.
private func applyStashOid(gitDir: String, stashOid: OID) throws -> StashApplyResult {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: stashOid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("stash is not a commit")
    }
    let wCommit = try parseCommit(oid: stashOid, data: data)

    guard !wCommit.parentIds.isEmpty else {
        throw MuonGitError.invalidObject("stash commit has no parents")
    }
    let baseOid = wCommit.parentIds[0]

    let baseEntries = try loadCommitTree(gitDir: gitDir, commitOid: baseOid)
    let stashEntries = try loadTreeEntries(gitDir: gitDir, treeOid: wCommit.treeId)

    let headOid = try resolveReference(gitDir: gitDir, name: "HEAD")
    let headEntries = try loadCommitTree(gitDir: gitDir, commitOid: headOid)

    let mergeResult = try mergeTreesContent(
        gitDir: gitDir,
        base: baseEntries,
        ours: headEntries,
        theirs: stashEntries
    )

    return StashApplyResult(hasConflicts: mergeResult.hasConflicts, files: mergeResult.files)
}
