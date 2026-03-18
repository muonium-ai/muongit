/// MuonGit - Tree object read/write
import Foundation

/// File mode for tree entries
public enum FileMode: UInt32, Sendable {
    case tree       = 0o040000
    case blob       = 0o100644
    case blobExe    = 0o100755
    case link       = 0o120000
    case gitlink    = 0o160000
}

/// A single entry in a tree object
public struct TreeEntry: Sendable {
    public let mode: UInt32
    public let name: String
    public let oid: OID

    public init(mode: UInt32, name: String, oid: OID) {
        self.mode = mode
        self.name = name
        self.oid = oid
    }

    /// Whether this entry is a subtree (directory)
    public var isTree: Bool { mode == FileMode.tree.rawValue }

    /// Whether this entry is a blob
    public var isBlob: Bool { mode == FileMode.blob.rawValue || mode == FileMode.blobExe.rawValue }
}

/// A parsed git tree object
public struct Tree: Sendable {
    public let oid: OID
    public let entries: [TreeEntry]
}

// MARK: - Parsing

/// Parse a tree object from its raw binary data
public func parseTree(oid: OID, data: Data) throws -> Tree {
    var entries: [TreeEntry] = []
    let bytes = Array(data)
    var i = 0

    while i < bytes.count {
        // Parse mode (octal digits until space)
        let modeStart = i
        while i < bytes.count && bytes[i] != 0x20 { // space
            i += 1
        }
        guard i < bytes.count else {
            throw MuonGitError.invalidObject("tree entry: missing space after mode")
        }
        let modeStr = String(bytes: Array(bytes[modeStart..<i]), encoding: .utf8) ?? ""
        guard let mode = UInt32(modeStr, radix: 8) else {
            throw MuonGitError.invalidObject("tree entry: invalid mode '\(modeStr)'")
        }
        i += 1 // skip space

        // Parse name (until null byte)
        let nameStart = i
        while i < bytes.count && bytes[i] != 0x00 {
            i += 1
        }
        guard i < bytes.count else {
            throw MuonGitError.invalidObject("tree entry: missing null after name")
        }
        let name = String(bytes: Array(bytes[nameStart..<i]), encoding: .utf8) ?? ""
        i += 1 // skip null

        // Read 20-byte raw OID
        guard i + 20 <= bytes.count else {
            throw MuonGitError.invalidObject("tree entry: truncated OID")
        }
        let entryOid = OID(raw: Array(bytes[i..<i+20]))
        i += 20

        entries.append(TreeEntry(mode: mode, name: name, oid: entryOid))
    }

    return Tree(oid: oid, entries: entries)
}

// MARK: - Serialization

/// Serialize tree entries to raw binary data (without the object header)
/// Entries are sorted by name with tree-sorting rules (directories sort as name + "/")
public func serializeTree(entries: [TreeEntry]) -> Data {
    let sorted = entries.sorted { a, b in
        let aKey = a.isTree ? "\(a.name)/" : a.name
        let bKey = b.isTree ? "\(b.name)/" : b.name
        return aKey < bKey
    }

    var result = Data()
    for entry in sorted {
        // Mode as octal string
        let modeStr = String(entry.mode, radix: 8)
        result.append(contentsOf: Array(modeStr.utf8))
        result.append(0x20) // space
        // Name
        result.append(contentsOf: Array(entry.name.utf8))
        result.append(0x00) // null
        // Raw 20-byte OID
        result.append(contentsOf: entry.oid.raw)
    }
    return result
}
