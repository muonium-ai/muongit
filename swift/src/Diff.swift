/// MuonGit - Tree-to-tree, index-to-workdir diff and diff formatting
/// Parity: libgit2 src/libgit2/diff.c, diff_print.c
import Foundation

/// The kind of change for a diff entry
public enum DiffStatus: Sendable {
    case added
    case deleted
    case modified
}

/// A single diff delta between two trees
public struct DiffDelta: Sendable {
    public let status: DiffStatus
    public let oldEntry: TreeEntry?
    public let newEntry: TreeEntry?
    public let path: String
}

/// Compute the diff between two trees.
/// Both entry lists should be sorted by name (as git trees are).
public func diffTrees(oldEntries: [TreeEntry], newEntries: [TreeEntry]) -> [DiffDelta] {
    var deltas: [DiffDelta] = []
    var oi = 0
    var ni = 0

    while oi < oldEntries.count && ni < newEntries.count {
        let old = oldEntries[oi]
        let new = newEntries[ni]

        if old.name < new.name {
            deltas.append(DiffDelta(status: .deleted, oldEntry: old, newEntry: nil, path: old.name))
            oi += 1
        } else if old.name > new.name {
            deltas.append(DiffDelta(status: .added, oldEntry: nil, newEntry: new, path: new.name))
            ni += 1
        } else {
            if old.oid != new.oid || old.mode != new.mode {
                deltas.append(DiffDelta(status: .modified, oldEntry: old, newEntry: new, path: old.name))
            }
            oi += 1
            ni += 1
        }
    }

    while oi < oldEntries.count {
        let old = oldEntries[oi]
        deltas.append(DiffDelta(status: .deleted, oldEntry: old, newEntry: nil, path: old.name))
        oi += 1
    }

    while ni < newEntries.count {
        let new = newEntries[ni]
        deltas.append(DiffDelta(status: .added, oldEntry: nil, newEntry: new, path: new.name))
        ni += 1
    }

    return deltas
}

/// Compute the diff between the index (staging area) and the working directory.
/// Returns deltas for modified, deleted, and new (untracked) files.
public func diffIndexToWorkdir(gitDir: String, workdir: String) throws -> [DiffDelta] {
    let index = try readIndex(gitDir: gitDir)
    var deltas: [DiffDelta] = []
    let fm = FileManager.default

    let indexedPaths = Set(index.entries.map { $0.path })

    // Check each index entry against the working directory
    for entry in index.entries {
        let filePath = (workdir as NSString).appendingPathComponent(entry.path)
        if !fm.fileExists(atPath: filePath) {
            deltas.append(DiffDelta(
                status: .deleted,
                oldEntry: indexEntryToTreeEntry(entry),
                newEntry: nil,
                path: entry.path
            ))
        } else {
            let attrs = try fm.attributesOfItem(atPath: filePath)
            let fileSize = (attrs[.size] as? UInt64) ?? 0

            var modified = UInt32(fileSize) != entry.fileSize
            if !modified {
                let content = try Data(contentsOf: URL(fileURLWithPath: filePath))
                let oid = OID.hash(type: .blob, data: Array(content))
                modified = oid != entry.oid
            }

            if modified {
                let content = try Data(contentsOf: URL(fileURLWithPath: filePath))
                let workdirOid = OID.hash(type: .blob, data: Array(content))
                let workdirMode: UInt32 = fm.isExecutableFile(atPath: filePath) ? FileMode.blobExe.rawValue : FileMode.blob.rawValue
                deltas.append(DiffDelta(
                    status: .modified,
                    oldEntry: indexEntryToTreeEntry(entry),
                    newEntry: TreeEntry(mode: workdirMode, name: entry.path, oid: workdirOid),
                    path: entry.path
                ))
            }
        }
    }

    // Find new (untracked) files
    var newFiles: [String] = []
    collectDiffFiles(dir: workdir, workdir: workdir, gitDir: gitDir, indexed: indexedPaths, result: &newFiles)
    newFiles.sort()

    for relPath in newFiles {
        let filePath = (workdir as NSString).appendingPathComponent(relPath)
        let content = try Data(contentsOf: URL(fileURLWithPath: filePath))
        let oid = OID.hash(type: .blob, data: Array(content))
        let mode: UInt32 = fm.isExecutableFile(atPath: filePath) ? FileMode.blobExe.rawValue : FileMode.blob.rawValue
        deltas.append(DiffDelta(
            status: .added,
            oldEntry: nil,
            newEntry: TreeEntry(mode: mode, name: relPath, oid: oid),
            path: relPath
        ))
    }

    return deltas
}

private func indexEntryToTreeEntry(_ entry: IndexEntry) -> TreeEntry {
    TreeEntry(mode: entry.mode, name: entry.path, oid: entry.oid)
}

private func collectDiffFiles(dir: String, workdir: String, gitDir: String, indexed: Set<String>, result: inout [String]) {
    let fm = FileManager.default
    guard let items = try? fm.contentsOfDirectory(atPath: dir) else { return }

    for item in items {
        if item == ".git" { continue }
        let fullPath = (dir as NSString).appendingPathComponent(item)

        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: fullPath, isDirectory: &isDir) else { continue }

        if isDir.boolValue {
            collectDiffFiles(dir: fullPath, workdir: workdir, gitDir: gitDir, indexed: indexed, result: &result)
        } else {
            let prefix = workdir.hasSuffix("/") ? workdir : workdir + "/"
            if fullPath.hasPrefix(prefix) {
                let relative = String(fullPath.dropFirst(prefix.count))
                if !indexed.contains(relative) {
                    result.append(relative)
                }
            }
        }
    }
}

// MARK: - Diff Formatting (patch and stat)

/// A single edit operation in a line-level diff.
public enum EditKind: Sendable {
    case equal
    case insert
    case delete
}

/// A line-level edit.
public struct Edit: Sendable {
    public let kind: EditKind
    public let oldLine: Int  // 1-based, 0 if insert
    public let newLine: Int  // 1-based, 0 if delete
    public let text: String
}

/// A unified diff hunk.
public struct DiffHunk: Sendable {
    public let oldStart: Int
    public let oldCount: Int
    public let newStart: Int
    public let newCount: Int
    public let edits: [Edit]
}

/// Compute a line diff between two texts using LCS.
public func diffLines(oldText: String, newText: String) -> [Edit] {
    let oldLines = oldText.isEmpty ? [String]() : oldText.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
    let newLines = newText.isEmpty ? [String]() : newText.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)

    let n = oldLines.count
    let m = newLines.count

    // LCS DP table
    var dp = Array(repeating: Array(repeating: 0, count: m + 1), count: n + 1)
    for i in 1...max(n, 1) {
        guard i <= n else { break }
        for j in 1...max(m, 1) {
            guard j <= m else { break }
            if oldLines[i - 1] == newLines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1
            } else {
                dp[i][j] = max(dp[i - 1][j], dp[i][j - 1])
            }
        }
    }

    // Backtrack
    var edits: [Edit] = []
    var i = n
    var j = m

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && oldLines[i - 1] == newLines[j - 1] {
            edits.append(Edit(kind: .equal, oldLine: i, newLine: j, text: oldLines[i - 1]))
            i -= 1; j -= 1
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            edits.append(Edit(kind: .insert, oldLine: 0, newLine: j, text: newLines[j - 1]))
            j -= 1
        } else {
            edits.append(Edit(kind: .delete, oldLine: i, newLine: 0, text: oldLines[i - 1]))
            i -= 1
        }
    }

    return edits.reversed()
}

/// Group edits into unified diff hunks with the given context lines.
public func makeHunks(edits: [Edit], context: Int = 3) -> [DiffHunk] {
    let changeIndices = edits.enumerated().compactMap { $0.element.kind != .equal ? $0.offset : nil }
    if changeIndices.isEmpty { return [] }

    var groups: [(Int, Int)] = []
    var ci = 0
    while ci < changeIndices.count {
        let start = changeIndices[ci]
        var end = start
        while ci + 1 < changeIndices.count && changeIndices[ci + 1] <= end + 2 * context + 1 {
            ci += 1
            end = changeIndices[ci]
        }
        groups.append((start, end))
        ci += 1
    }

    var hunks: [DiffHunk] = []
    for (firstChange, lastChange) in groups {
        let hunkStart = firstChange > context ? firstChange - context : 0
        let hunkEnd = min(lastChange + context + 1, edits.count)
        let hunkEdits = Array(edits[hunkStart..<hunkEnd])

        var oldStart = 0, newStart = 0, oldCount = 0, newCount = 0
        for (idx, edit) in hunkEdits.enumerated() {
            if idx == 0 {
                switch edit.kind {
                case .equal, .delete: oldStart = edit.oldLine
                case .insert:
                    oldStart = edit.newLine
                    for e in hunkEdits where e.oldLine > 0 { oldStart = e.oldLine; break }
                }
                switch edit.kind {
                case .equal, .insert: newStart = edit.newLine
                case .delete:
                    newStart = edit.oldLine
                    for e in hunkEdits where e.newLine > 0 { newStart = e.newLine; break }
                }
            }
            switch edit.kind {
            case .equal: oldCount += 1; newCount += 1
            case .delete: oldCount += 1
            case .insert: newCount += 1
            }
        }

        hunks.append(DiffHunk(oldStart: oldStart, oldCount: oldCount, newStart: newStart, newCount: newCount, edits: hunkEdits))
    }

    return hunks
}

/// Format a diff as a unified patch string.
public func formatPatch(oldPath: String, newPath: String, oldText: String, newText: String, context: Int = 3) -> String {
    let edits = diffLines(oldText: oldText, newText: newText)
    let hunks = makeHunks(edits: edits, context: context)

    if hunks.isEmpty { return "" }

    var out = "--- a/\(oldPath)\n+++ b/\(newPath)\n"
    for hunk in hunks {
        out += "@@ -\(hunk.oldStart),\(hunk.oldCount) +\(hunk.newStart),\(hunk.newCount) @@\n"
        for edit in hunk.edits {
            switch edit.kind {
            case .equal: out += " \(edit.text)\n"
            case .delete: out += "-\(edit.text)\n"
            case .insert: out += "+\(edit.text)\n"
            }
        }
    }
    return out
}

/// A stat entry for a single file.
public struct DiffStatEntry: Sendable {
    public let path: String
    public let insertions: Int
    public let deletions: Int
}

/// Compute diff stats for a single file.
public func diffStat(path: String, oldText: String, newText: String) -> DiffStatEntry {
    let edits = diffLines(oldText: oldText, newText: newText)
    let insertions = edits.filter { $0.kind == .insert }.count
    let deletions = edits.filter { $0.kind == .delete }.count
    return DiffStatEntry(path: path, insertions: insertions, deletions: deletions)
}

/// Format stat entries as a diffstat string (like `git diff --stat`).
public func formatStat(stats: [DiffStatEntry]) -> String {
    if stats.isEmpty { return "" }

    let maxPathLen = stats.map { $0.path.count }.max() ?? 0
    let barWidth = 40

    var out = ""
    var totalInsertions = 0
    var totalDeletions = 0

    for stat in stats {
        let changes = stat.insertions + stat.deletions
        totalInsertions += stat.insertions
        totalDeletions += stat.deletions

        let (plusCount, minusCount): (Int, Int)
        if changes > 0 {
            let totalBars = min(changes, barWidth)
            let pb = Int((Double(stat.insertions) / Double(changes) * Double(totalBars)).rounded())
            plusCount = pb
            minusCount = totalBars - pb
        } else {
            plusCount = 0; minusCount = 0
        }

        let paddedPath = stat.path.padding(toLength: maxPathLen, withPad: " ", startingAt: 0)
        let changesStr = String(changes).leftPadding(toLength: 5)
        out += " \(paddedPath) | \(changesStr) \(String(repeating: "+", count: plusCount))\(String(repeating: "-", count: minusCount))\n"
    }

    let fileWord = stats.count == 1 ? "file" : "files"
    out += " \(stats.count) \(fileWord) changed, \(totalInsertions) insertions(+), \(totalDeletions) deletions(-)\n"
    return out
}

private extension String {
    func leftPadding(toLength length: Int) -> String {
        if self.count >= length { return self }
        return String(repeating: " ", count: length - self.count) + self
    }
}
