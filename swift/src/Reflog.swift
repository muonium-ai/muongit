/// MuonGit - Reflog read/write
/// Parity: libgit2 src/libgit2/reflog.c
import Foundation

/// A single reflog entry
public struct ReflogEntry: Sendable {
    public let oldOid: OID
    public let newOid: OID
    public let committer: Signature
    public let message: String
}

// MARK: - Reading

/// Read the reflog for a given reference name.
/// Reflog files live at .git/logs/<refname>
public func readReflog(gitDir: String, refName: String) throws -> [ReflogEntry] {
    let logPath = (gitDir as NSString).appendingPathComponent("logs/\(refName)")
    guard FileManager.default.fileExists(atPath: logPath) else {
        return []
    }
    let content = try String(contentsOfFile: logPath, encoding: .utf8)
    return parseReflog(content)
}

/// Parse reflog file content into entries
func parseReflog(_ content: String) -> [ReflogEntry] {
    var entries: [ReflogEntry] = []

    for line in content.components(separatedBy: .newlines) {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { continue }

        // Format: "<old> <new> <name> <<email>> <time> <offset>\t<message>"
        guard let tabIndex = trimmed.firstIndex(of: "\t") else { continue }
        let sigPart = String(trimmed[trimmed.startIndex..<tabIndex])
        let message = String(trimmed[trimmed.index(after: tabIndex)...])

        let parts = sigPart.split(separator: " ", maxSplits: 2)
        guard parts.count >= 3 else { continue }

        let oldOid = OID(hex: String(parts[0]))
        let newOid = OID(hex: String(parts[1]))
        let sigStr = String(parts[2])
        let committer = parseSignature(sigStr)

        entries.append(ReflogEntry(oldOid: oldOid, newOid: newOid, committer: committer, message: message))
    }

    return entries
}

// MARK: - Writing

/// Append an entry to the reflog for a given reference.
/// Creates the log file and parent directories if needed.
public func appendReflog(
    gitDir: String,
    refName: String,
    oldOid: OID,
    newOid: OID,
    committer: Signature,
    message: String
) throws {
    let logPath = (gitDir as NSString).appendingPathComponent("logs/\(refName)")
    let parentDir = (logPath as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parentDir, withIntermediateDirectories: true)

    let line = formatReflogEntry(oldOid: oldOid, newOid: newOid, committer: committer, message: message)

    if FileManager.default.fileExists(atPath: logPath) {
        let handle = try FileHandle(forWritingTo: URL(fileURLWithPath: logPath))
        handle.seekToEndOfFile()
        handle.write(Data(line.utf8))
        handle.closeFile()
    } else {
        try line.write(toFile: logPath, atomically: true, encoding: .utf8)
    }
}

/// Format a single reflog entry line
func formatReflogEntry(oldOid: OID, newOid: OID, committer: Signature, message: String) -> String {
    return "\(oldOid.hex) \(newOid.hex) \(formatSignature(committer))\t\(message)\n"
}
