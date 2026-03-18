/// MuonGit - Working directory status
/// Parity: libgit2 src/libgit2/status.c
import Foundation

/// Status of a file in the working directory
public enum FileStatus: Sendable {
    case deleted
    case new
    case modified
}

/// A single status entry
public struct StatusEntry: Sendable {
    public let path: String
    public let status: FileStatus
}

/// Compute the working directory status by comparing the index against the workdir.
public func workdirStatus(gitDir: String, workdir: String) throws -> [StatusEntry] {
    let index = try readIndex(gitDir: gitDir)
    var entries: [StatusEntry] = []
    let fm = FileManager.default

    let indexedPaths = Set(index.entries.map { $0.path })

    // Check each index entry against the working directory
    for entry in index.entries {
        let filePath = (workdir as NSString).appendingPathComponent(entry.path)
        if !fm.fileExists(atPath: filePath) {
            entries.append(StatusEntry(path: entry.path, status: .deleted))
        } else if try isModified(filePath: filePath, entry: entry) {
            entries.append(StatusEntry(path: entry.path, status: .modified))
        }
    }

    // Find new (untracked) files
    var newFiles: [String] = []
    collectFiles(dir: workdir, workdir: workdir, gitDir: gitDir, indexed: indexedPaths, result: &newFiles)
    newFiles.sort()
    for path in newFiles {
        entries.append(StatusEntry(path: path, status: .new))
    }

    return entries
}

private func isModified(filePath: String, entry: IndexEntry) throws -> Bool {
    let attrs = try FileManager.default.attributesOfItem(atPath: filePath)
    let fileSize = (attrs[.size] as? UInt64) ?? 0

    if UInt32(fileSize) != entry.fileSize {
        return true
    }

    let content = try Data(contentsOf: URL(fileURLWithPath: filePath))
    let oid = OID.hash(type: .blob, data: Array(content))
    return oid != entry.oid
}

private func collectFiles(dir: String, workdir: String, gitDir: String, indexed: Set<String>, result: inout [String]) {
    let fm = FileManager.default
    guard let items = try? fm.contentsOfDirectory(atPath: dir) else { return }

    for item in items {
        if item == ".git" { continue }
        let fullPath = (dir as NSString).appendingPathComponent(item)

        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: fullPath, isDirectory: &isDir) else { continue }

        if isDir.boolValue {
            collectFiles(dir: fullPath, workdir: workdir, gitDir: gitDir, indexed: indexed, result: &result)
        } else {
            // Compute relative path
            let workdirNS = workdir as NSString
            let prefix = workdirNS.hasSuffix("/") ? workdir : workdir + "/"
            if fullPath.hasPrefix(prefix) {
                let relative = String(fullPath.dropFirst(prefix.count))
                if !indexed.contains(relative) {
                    result.append(relative)
                }
            }
        }
    }
}
