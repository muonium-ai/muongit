/// MuonGit - Blame support
/// Parity: libgit2 src/libgit2/blame.c, blame_git.c
import Foundation

/// Options controlling blame behavior
public struct BlameOptions: Sendable {
    /// Restrict blame to this commit (newest). Default: HEAD.
    public var newestCommit: OID?
    /// Stop blaming at this commit. Default: root.
    public var oldestCommit: OID?
    /// Only blame lines in [minLine, maxLine] (1-based, inclusive). 0 = all.
    public var minLine: Int
    public var maxLine: Int

    public init(newestCommit: OID? = nil, oldestCommit: OID? = nil, minLine: Int = 0, maxLine: Int = 0) {
        self.newestCommit = newestCommit
        self.oldestCommit = oldestCommit
        self.minLine = minLine
        self.maxLine = maxLine
    }
}

/// A hunk of lines attributed to a single commit
public struct BlameHunk: Sendable {
    /// Number of lines in this hunk
    public let linesInHunk: Int
    /// The commit that introduced these lines
    public let finalCommitId: OID
    /// 1-based start line in the final file
    public let finalStartLineNumber: Int
    /// Author signature from the blamed commit
    public let finalSignature: Signature?
    /// The original commit (same as final unless tracking copies)
    public let origCommitId: OID
    /// 1-based start line in the original file
    public let origStartLineNumber: Int
    /// Original path if different from blamed path
    public let origPath: String?
    /// True if this hunk is at the oldest_commit boundary
    public let boundary: Bool
}

/// Result of a blame operation
public struct BlameResult: Sendable {
    /// The path that was blamed
    public let path: String
    /// Blame hunks covering all lines
    public let hunks: [BlameHunk]
    /// Total line count in the file
    public let lineCount: Int

    /// Number of hunks
    public var hunkCount: Int { hunks.count }

    /// Get hunk by 0-based index
    public func hunkByIndex(_ index: Int) -> BlameHunk? {
        guard index >= 0, index < hunks.count else { return nil }
        return hunks[index]
    }

    /// Get the hunk that covers a specific 1-based line number
    public func hunkByLine(_ line: Int) -> BlameHunk? {
        guard line >= 1, line <= lineCount else { return nil }
        for hunk in hunks {
            let end = hunk.finalStartLineNumber + hunk.linesInHunk
            if line >= hunk.finalStartLineNumber && line < end {
                return hunk
            }
        }
        return nil
    }
}

/// Blame a file, attributing each line to the commit that last changed it.
public func blameFile(
    gitDir: String,
    path: String,
    options: BlameOptions? = nil
) throws -> BlameResult {
    let opts = options ?? BlameOptions()

    // Resolve starting commit
    let startOid: OID
    if let newest = opts.newestCommit {
        startOid = newest
    } else {
        startOid = try resolveReference(gitDir: gitDir, name: "HEAD")
    }

    // Read file content at starting commit
    let fileContent = try readBlobAtCommit(gitDir: gitDir, commitOid: startOid, path: path)
    let lines = fileContent.isEmpty ? [String]() : fileContent.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
    let totalLines = lines.count

    if totalLines == 0 {
        return BlameResult(path: path, hunks: [], lineCount: 0)
    }

    let minLine = opts.minLine > 0 ? opts.minLine : 1
    let maxLine = opts.maxLine > 0 ? min(opts.maxLine, totalLines) : totalLines

    // Per-line tracking: (commitOid, origLine1Based)
    var lineOwners: [(OID, Int)?] = Array(repeating: nil, count: totalLines)

    var currentOid = startOid
    var currentContent = fileContent
    var remaining = maxLine - minLine + 1
    let maxDepth = 10000

    for _ in 0..<maxDepth {
        if remaining <= 0 { break }

        let commit = try readCommit(gitDir: gitDir, oid: currentOid)

        if commit.parentIds.isEmpty {
            // Root commit — attribute all remaining lines
            for i in 0..<totalLines {
                let line1 = i + 1
                if lineOwners[i] == nil && line1 >= minLine && line1 <= maxLine {
                    lineOwners[i] = (currentOid, line1)
                }
            }
            break
        }

        // Check oldest_commit boundary
        if let oldest = opts.oldestCommit, currentOid == oldest {
            for i in 0..<totalLines {
                let line1 = i + 1
                if lineOwners[i] == nil && line1 >= minLine && line1 <= maxLine {
                    lineOwners[i] = (currentOid, line1)
                }
            }
            break
        }

        let parentOid = commit.parentIds[0]

        // Read file at parent
        let parentContent: String
        do {
            parentContent = try readBlobAtCommit(gitDir: gitDir, commitOid: parentOid, path: path)
        } catch {
            // File didn't exist in parent
            for i in 0..<totalLines {
                let line1 = i + 1
                if lineOwners[i] == nil && line1 >= minLine && line1 <= maxLine {
                    lineOwners[i] = (currentOid, line1)
                }
            }
            break
        }

        if parentContent == currentContent {
            currentOid = parentOid
            continue
        }

        // Diff parent vs current
        let edits = diffLines(oldText: parentContent, newText: currentContent)

        for edit in edits {
            if edit.kind == .insert && edit.newLine > 0 {
                let lineIdx = edit.newLine - 1
                if lineIdx < totalLines {
                    let line1 = lineIdx + 1
                    if lineOwners[lineIdx] == nil && line1 >= minLine && line1 <= maxLine {
                        lineOwners[lineIdx] = (currentOid, line1)
                        remaining -= 1
                    }
                }
            }
        }

        currentOid = parentOid
        currentContent = parentContent
    }

    // Attribute unowned lines to start commit
    for i in 0..<totalLines {
        let line1 = i + 1
        if lineOwners[i] == nil && line1 >= minLine && line1 <= maxLine {
            lineOwners[i] = (startOid, line1)
        }
    }

    // Build hunks from consecutive lines with same commit
    var hunks: [BlameHunk] = []
    var i = minLine - 1

    while i < maxLine {
        let (commitId, origLine) = lineOwners[i] ?? (startOid, i + 1)
        let startLine = i + 1
        var count = 1

        while i + count < maxLine {
            if let (nextOid, _) = lineOwners[i + count], nextOid == commitId {
                count += 1
            } else {
                break
            }
        }

        // Load author signature
        let sig: Signature?
        if let c = try? readCommit(gitDir: gitDir, oid: commitId) {
            sig = c.author
        } else {
            sig = nil
        }

        let isBoundary = opts.oldestCommit.map { $0 == commitId } ?? false

        hunks.append(BlameHunk(
            linesInHunk: count,
            finalCommitId: commitId,
            finalStartLineNumber: startLine,
            finalSignature: sig,
            origCommitId: commitId,
            origStartLineNumber: origLine,
            origPath: nil,
            boundary: isBoundary
        ))

        i += count
    }

    return BlameResult(path: path, hunks: hunks, lineCount: totalLines)
}

// MARK: - Internal helpers

private func readCommit(gitDir: String, oid: OID) throws -> Commit {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: oid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("expected commit, got \(objType)")
    }
    return try parseCommit(oid: oid, data: data)
}

private func readBlobAtCommit(gitDir: String, commitOid: OID, path: String) throws -> String {
    let commit = try readCommit(gitDir: gitDir, oid: commitOid)
    let (treeType, treeData) = try readLooseObject(gitDir: gitDir, oid: commit.treeId)
    guard treeType == .tree else {
        throw MuonGitError.invalidObject("expected tree")
    }
    let tree = try parseTree(oid: commit.treeId, data: treeData)

    let entry = try findTreeEntryByPath(gitDir: gitDir, entries: tree.entries, path: path)

    let (blobType, blobData) = try readLooseObject(gitDir: gitDir, oid: entry.oid)
    guard blobType == .blob else {
        throw MuonGitError.invalidObject("expected blob")
    }
    return String(data: blobData, encoding: .utf8) ?? ""
}

private func findTreeEntryByPath(gitDir: String, entries: [TreeEntry], path: String) throws -> TreeEntry {
    let parts = path.split(separator: "/", maxSplits: 1).map(String.init)
    let name = parts[0]

    guard let entry = entries.first(where: { $0.name == name }) else {
        throw MuonGitError.notFound("path not found: \(path)")
    }

    if parts.count == 1 {
        return entry
    }

    // Subdirectory — recurse
    let (subType, subData) = try readLooseObject(gitDir: gitDir, oid: entry.oid)
    guard subType == .tree else {
        throw MuonGitError.invalidObject("expected tree for directory \(name)")
    }
    let subTree = try parseTree(oid: entry.oid, data: subData)
    return try findTreeEntryByPath(gitDir: gitDir, entries: subTree.entries, path: parts[1])
}
