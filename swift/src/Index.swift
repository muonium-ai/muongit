/// MuonGit - Git index (staging area) read/write
/// Parity: libgit2 src/libgit2/index.c
import Foundation

private let indexSignature: [UInt8] = [0x44, 0x49, 0x52, 0x43] // "DIRC"
private let indexVersion: UInt32 = 2
private let entryFixedSize = 62 // 10*4 + 20 + 2

/// A single entry in the git index
public struct IndexEntry: Sendable {
    public let ctimeSecs: UInt32
    public let ctimeNanos: UInt32
    public let mtimeSecs: UInt32
    public let mtimeNanos: UInt32
    public let dev: UInt32
    public let ino: UInt32
    public let mode: UInt32
    public let uid: UInt32
    public let gid: UInt32
    public let fileSize: UInt32
    public let oid: OID
    public let flags: UInt16
    public let path: String

    public init(ctimeSecs: UInt32 = 0, ctimeNanos: UInt32 = 0,
                mtimeSecs: UInt32 = 0, mtimeNanos: UInt32 = 0,
                dev: UInt32 = 0, ino: UInt32 = 0,
                mode: UInt32, uid: UInt32 = 0, gid: UInt32 = 0,
                fileSize: UInt32 = 0, oid: OID, flags: UInt16 = 0,
                path: String) {
        self.ctimeSecs = ctimeSecs
        self.ctimeNanos = ctimeNanos
        self.mtimeSecs = mtimeSecs
        self.mtimeNanos = mtimeNanos
        self.dev = dev
        self.ino = ino
        self.mode = mode
        self.uid = uid
        self.gid = gid
        self.fileSize = fileSize
        self.oid = oid
        self.flags = flags
        self.path = path
    }
}

/// The parsed git index
public struct Index: Sendable {
    public var version: UInt32
    public var entries: [IndexEntry]

    public init(version: UInt32 = 2, entries: [IndexEntry] = []) {
        self.version = version
        self.entries = entries
    }

    public mutating func add(_ entry: IndexEntry) {
        if let idx = entries.firstIndex(where: { $0.path == entry.path }) {
            entries[idx] = entry
        } else {
            entries.append(entry)
            entries.sort { $0.path < $1.path }
        }
    }

    @discardableResult
    public mutating func remove(path: String) -> Bool {
        if let idx = entries.firstIndex(where: { $0.path == path }) {
            entries.remove(at: idx)
            return true
        }
        return false
    }

    public func find(path: String) -> IndexEntry? {
        entries.first { $0.path == path }
    }
}

// MARK: - Reading

/// Read and parse the git index file.
public func readIndex(gitDir: String) throws -> Index {
    let indexPath = (gitDir as NSString).appendingPathComponent("index")
    guard FileManager.default.fileExists(atPath: indexPath) else {
        return Index()
    }
    let data = try Data(contentsOf: URL(fileURLWithPath: indexPath))
    return try parseIndex(Array(data))
}

private func readU32(_ data: [UInt8], _ offset: Int) -> UInt32 {
    return (UInt32(data[offset]) << 24) | (UInt32(data[offset+1]) << 16) |
           (UInt32(data[offset+2]) << 8) | UInt32(data[offset+3])
}

private func readU16(_ data: [UInt8], _ offset: Int) -> UInt16 {
    return (UInt16(data[offset]) << 8) | UInt16(data[offset+1])
}

func parseIndex(_ data: [UInt8]) throws -> Index {
    guard data.count >= 12 else {
        throw MuonGitError.invalidObject("index too short")
    }

    // Validate signature
    guard data[0] == 0x44, data[1] == 0x49, data[2] == 0x52, data[3] == 0x43 else {
        throw MuonGitError.invalidObject("bad index signature")
    }

    let version = readU32(data, 4)
    guard version == 2 else {
        throw MuonGitError.invalidObject("unsupported index version \(version)")
    }

    let entryCount = Int(readU32(data, 8))

    // Validate checksum
    guard data.count >= 20 else {
        throw MuonGitError.invalidObject("index too short for checksum")
    }
    let content = Array(data[0..<data.count - 20])
    let storedChecksum = Array(data[data.count - 20..<data.count])
    let computed = SHA1.hash(content)
    guard computed == storedChecksum else {
        throw MuonGitError.invalidObject("index checksum mismatch")
    }

    var entries: [IndexEntry] = []
    var offset = 12

    for _ in 0..<entryCount {
        guard offset + entryFixedSize <= content.count else {
            throw MuonGitError.invalidObject("index truncated")
        }

        let ctimeSecs = readU32(data, offset)
        let ctimeNanos = readU32(data, offset + 4)
        let mtimeSecs = readU32(data, offset + 8)
        let mtimeNanos = readU32(data, offset + 12)
        let dev = readU32(data, offset + 16)
        let ino = readU32(data, offset + 20)
        let mode = readU32(data, offset + 24)
        let uid = readU32(data, offset + 28)
        let gid = readU32(data, offset + 32)
        let fileSize = readU32(data, offset + 36)

        let oidBytes = Array(data[offset + 40..<offset + 60])
        let oid = OID(raw: oidBytes)
        let flags = readU16(data, offset + 60)

        // Read null-terminated path
        let pathStart = offset + entryFixedSize
        var pathEnd = pathStart
        while pathEnd < content.count && data[pathEnd] != 0 {
            pathEnd += 1
        }
        guard pathEnd < content.count else {
            throw MuonGitError.invalidObject("unterminated path in index")
        }

        let pathBytes = Array(data[pathStart..<pathEnd])
        guard let path = String(bytes: pathBytes, encoding: .utf8) else {
            throw MuonGitError.invalidObject("invalid UTF-8 path in index")
        }

        // Compute padding to 8-byte alignment
        let entryLen = entryFixedSize + path.utf8.count + 1
        let paddedLen = (entryLen + 7) & ~7
        offset += paddedLen

        entries.append(IndexEntry(
            ctimeSecs: ctimeSecs, ctimeNanos: ctimeNanos,
            mtimeSecs: mtimeSecs, mtimeNanos: mtimeNanos,
            dev: dev, ino: ino, mode: mode, uid: uid, gid: gid,
            fileSize: fileSize, oid: oid, flags: flags, path: path
        ))
    }

    return Index(version: version, entries: entries)
}

// MARK: - Writing

/// Write the index to the git directory.
public func writeIndex(gitDir: String, index: Index) throws {
    let data = serializeIndex(index)
    let indexPath = (gitDir as NSString).appendingPathComponent("index")
    try Data(data).write(to: URL(fileURLWithPath: indexPath))
}

func serializeIndex(_ index: Index) -> [UInt8] {
    var buf: [UInt8] = []

    // Header
    buf.append(contentsOf: indexSignature)
    buf.append(contentsOf: writeU32(index.version))

    // Sort entries by path
    let sorted = index.entries.sorted { $0.path < $1.path }
    buf.append(contentsOf: writeU32(UInt32(sorted.count)))

    for entry in sorted {
        buf.append(contentsOf: writeU32(entry.ctimeSecs))
        buf.append(contentsOf: writeU32(entry.ctimeNanos))
        buf.append(contentsOf: writeU32(entry.mtimeSecs))
        buf.append(contentsOf: writeU32(entry.mtimeNanos))
        buf.append(contentsOf: writeU32(entry.dev))
        buf.append(contentsOf: writeU32(entry.ino))
        buf.append(contentsOf: writeU32(entry.mode))
        buf.append(contentsOf: writeU32(entry.uid))
        buf.append(contentsOf: writeU32(entry.gid))
        buf.append(contentsOf: writeU32(entry.fileSize))
        buf.append(contentsOf: entry.oid.raw)

        // Flags: lower 12 bits = min(path_len, 0xFFF), upper bits from entry
        let nameLen = UInt16(min(entry.path.utf8.count, 0xFFF))
        let flags = (entry.flags & 0xF000) | nameLen
        buf.append(contentsOf: writeU16(flags))

        // Path + null padding to 8-byte alignment
        buf.append(contentsOf: Array(entry.path.utf8))
        let entryLen = entryFixedSize + entry.path.utf8.count + 1
        let paddedLen = (entryLen + 7) & ~7
        let padCount = paddedLen - entryFixedSize - entry.path.utf8.count
        buf.append(contentsOf: [UInt8](repeating: 0, count: padCount))
    }

    // Checksum
    let checksum = SHA1.hash(buf)
    buf.append(contentsOf: checksum)

    return buf
}

private func writeU32(_ value: UInt32) -> [UInt8] {
    return [
        UInt8((value >> 24) & 0xFF),
        UInt8((value >> 16) & 0xFF),
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
}

private func writeU16(_ value: UInt16) -> [UInt8] {
    return [
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
}
