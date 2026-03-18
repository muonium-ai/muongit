/// Reference reading for the git reference database
/// Parity: libgit2 src/libgit2/refs.c, src/libgit2/refdb_fs.c
import Foundation

// MARK: - Read Reference

/// Read a reference by name, returning its raw content (either a symbolic target or hex OID).
/// Checks loose ref file first, then packed-refs.
/// - Parameters:
///   - gitDir: Path to the .git directory
///   - name: Reference name (e.g. "HEAD", "refs/heads/main")
/// - Returns: The reference content, trimmed (e.g. "ref: refs/heads/main" or a hex OID)
public func readReference(gitDir: String, name: String) throws -> String {
    // Try loose ref first
    let loosePath = (gitDir as NSString).appendingPathComponent(name)
    if FileManager.default.fileExists(atPath: loosePath) {
        let content = try String(contentsOfFile: loosePath, encoding: .utf8)
        return content.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // Try packed-refs
    let packedPath = (gitDir as NSString).appendingPathComponent("packed-refs")
    if FileManager.default.fileExists(atPath: packedPath) {
        let content = try String(contentsOfFile: packedPath, encoding: .utf8)
        for line in content.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            // Skip comments and peel lines
            if trimmed.isEmpty || trimmed.hasPrefix("#") || trimmed.hasPrefix("^") {
                continue
            }
            let parts = trimmed.split(separator: " ", maxSplits: 1)
            if parts.count == 2 && String(parts[1]) == name {
                return String(parts[0])
            }
        }
    }

    throw MuonGitError.notFound("reference '\(name)' not found")
}

// MARK: - Resolve Reference

/// Resolve a reference to a final OID by following symbolic refs.
/// - Parameters:
///   - gitDir: Path to the .git directory
///   - name: Reference name (e.g. "HEAD", "refs/heads/main")
/// - Returns: The resolved OID
public func resolveReference(gitDir: String, name: String) throws -> OID {
    var current = name
    var maxDepth = 10 // prevent infinite loops

    while maxDepth > 0 {
        maxDepth -= 1
        let value = try readReference(gitDir: gitDir, name: current)

        if value.hasPrefix("ref: ") {
            // Symbolic ref - follow it
            current = String(value.dropFirst(5))
        } else {
            // Should be a hex OID
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard trimmed.count == OID.sha1HexLength else {
                throw MuonGitError.invalidObject("invalid OID in reference '\(current)': '\(trimmed)'")
            }
            return OID(hex: trimmed)
        }
    }

    throw MuonGitError.invalidObject("too many levels of symbolic references")
}

// MARK: - List References

/// List all references (loose + packed), returning pairs of (name, value).
/// - Parameter gitDir: Path to the .git directory
/// - Returns: Array of (refName, value) tuples
public func listReferences(gitDir: String) throws -> [(String, String)] {
    var refs = [String: String]() // name -> value

    // Collect packed refs first (loose refs override them)
    let packedPath = (gitDir as NSString).appendingPathComponent("packed-refs")
    if FileManager.default.fileExists(atPath: packedPath) {
        let content = try String(contentsOfFile: packedPath, encoding: .utf8)
        for line in content.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty || trimmed.hasPrefix("#") || trimmed.hasPrefix("^") {
                continue
            }
            let parts = trimmed.split(separator: " ", maxSplits: 1)
            if parts.count == 2 {
                refs[String(parts[1])] = String(parts[0])
            }
        }
    }

    // Walk loose refs under refs/
    let refsDir = (gitDir as NSString).appendingPathComponent("refs")
    let fm = FileManager.default
    if let enumerator = fm.enumerator(atPath: refsDir) {
        while let relativePath = enumerator.nextObject() as? String {
            let fullPath = (refsDir as NSString).appendingPathComponent(relativePath)
            var isDir: ObjCBool = false
            if fm.fileExists(atPath: fullPath, isDirectory: &isDir), !isDir.boolValue {
                let refName = "refs/\(relativePath)"
                if let content = try? String(contentsOfFile: fullPath, encoding: .utf8) {
                    refs[refName] = content.trimmingCharacters(in: .whitespacesAndNewlines)
                }
            }
        }
    }

    return refs.map { ($0.key, $0.value) }.sorted { $0.0 < $1.0 }
}

// MARK: - Write Reference

/// Write (create or update) a direct reference pointing to an OID.
/// Creates intermediate directories as needed.
public func writeReference(gitDir: String, name: String, oid: OID) throws {
    let refPath = (gitDir as NSString).appendingPathComponent(name)
    let parentDir = (refPath as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parentDir, withIntermediateDirectories: true)
    try "\(oid.hex)\n".write(toFile: refPath, atomically: true, encoding: .utf8)
}

/// Write (create or update) a symbolic reference.
public func writeSymbolicReference(gitDir: String, name: String, target: String) throws {
    let refPath = (gitDir as NSString).appendingPathComponent(name)
    let parentDir = (refPath as NSString).deletingLastPathComponent
    try FileManager.default.createDirectory(atPath: parentDir, withIntermediateDirectories: true)
    try "ref: \(target)\n".write(toFile: refPath, atomically: true, encoding: .utf8)
}

// MARK: - Delete Reference

/// Delete a loose reference file. Returns true if it existed and was deleted.
@discardableResult
public func deleteReference(gitDir: String, name: String) throws -> Bool {
    let refPath = (gitDir as NSString).appendingPathComponent(name)
    if FileManager.default.fileExists(atPath: refPath) {
        try FileManager.default.removeItem(atPath: refPath)
        return true
    }
    return false
}

// MARK: - Update Reference (compare-and-swap)

/// Update a reference only if its current value matches `oldOid`.
/// This is the atomic compare-and-swap primitive for refs.
/// Pass `OID.zero` for `oldOid` to require that the ref does not yet exist (create-only).
public func updateReference(gitDir: String, name: String, newOid: OID, oldOid: OID) throws {
    let refPath = (gitDir as NSString).appendingPathComponent(name)

    if oldOid.isZero {
        // Create-only: ref must not exist
        if FileManager.default.fileExists(atPath: refPath) {
            throw MuonGitError.conflict("reference '\(name)' already exists")
        }
    } else {
        // Must match current value
        let current = try readReference(gitDir: gitDir, name: name)
        guard current == oldOid.hex else {
            throw MuonGitError.conflict("reference '\(name)' expected \(oldOid.hex), got \(current)")
        }
    }

    try writeReference(gitDir: gitDir, name: name, oid: newOid)
}
