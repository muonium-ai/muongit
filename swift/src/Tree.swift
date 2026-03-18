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
    let count = data.count

    try data.withUnsafeBytes { (rawBuf: UnsafeRawBufferPointer) -> Void in
        guard let base = rawBuf.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return }
        var i = 0

        while i < count {
            // Parse mode (octal digits until space)
            let modeStart = i
            while i < count && base[i] != 0x20 {
                i += 1
            }
            guard i < count else {
                throw MuonGitError.invalidObject("tree entry: missing space after mode")
            }
            // Fast octal parse inline
            var mode: UInt32 = 0
            for j in modeStart..<i {
                mode = mode &* 8 &+ UInt32(base[j] &- 0x30)
            }
            i += 1 // skip space

            // Parse name (until null byte)
            let nameStart = i
            while i < count && base[i] != 0x00 {
                i += 1
            }
            guard i < count else {
                throw MuonGitError.invalidObject("tree entry: missing null after name")
            }
            let name = String(bytes: UnsafeBufferPointer(start: base + nameStart, count: i - nameStart), encoding: .utf8) ?? ""
            i += 1 // skip null

            // Read 20-byte raw OID
            guard i + 20 <= count else {
                throw MuonGitError.invalidObject("tree entry: truncated OID")
            }
            let entryOid = OID(unsafeRawPointer: base + i, count: 20)
            i += 20

            entries.append(TreeEntry(mode: mode, name: name, oid: entryOid))
        }
    }

    return Tree(oid: oid, entries: entries)
}

// MARK: - Serialization

/// Pre-computed octal mode bytes for common git modes
@inline(__always)
private func modeBytes(_ mode: UInt32) -> [UInt8]? {
    switch mode {
    case 0o100644: return [0x31,0x30,0x30,0x36,0x34,0x34] // "100644"
    case 0o040000: return [0x34,0x30,0x30,0x30,0x30]       // "40000"
    case 0o100755: return [0x31,0x30,0x30,0x37,0x35,0x35] // "100755"
    case 0o120000: return [0x31,0x32,0x30,0x30,0x30,0x30] // "120000"
    case 0o160000: return [0x31,0x36,0x30,0x30,0x30,0x30] // "160000"
    default: return nil
    }
}

/// Serialize tree entries to raw binary data (without the object header)
/// Entries are sorted by name with tree-sorting rules (directories sort as name + "/")
public func serializeTree(entries: [TreeEntry]) -> Data {
    // Pre-compute sort keys to avoid allocations in comparator
    var indexed = entries.enumerated().map { (idx, entry) -> (Int, String) in
        let key = entry.isTree ? entry.name + "/" : entry.name
        return (idx, key)
    }
    indexed.sort { $0.1 < $1.1 }
    let sorted = indexed.map { entries[$0.0] }

    // Pre-allocate: each entry ~28 bytes (6 mode + 1 space + ~12 name + 1 null + 20 oid)
    var result = Data(capacity: entries.count * 40)
    for entry in sorted {
        if let mb = modeBytes(entry.mode) {
            result.append(contentsOf: mb)
        } else {
            let modeStr = String(entry.mode, radix: 8)
            result.append(contentsOf: Array(modeStr.utf8))
        }
        result.append(0x20) // space
        result.append(contentsOf: Array(entry.name.utf8))
        result.append(0x00) // null
        result.append(contentsOf: entry.oid.raw)
    }
    return result
}
