/// MuonGit - Tree-to-tree and index-to-workdir diff
/// Parity: libgit2 src/libgit2/diff.c
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
