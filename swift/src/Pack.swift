/// MuonGit - Pack file object lookup and delta resolution
/// Parity: libgit2 src/libgit2/pack.c
import Foundation
import zlib

private let OBJ_COMMIT: UInt8 = 1
private let OBJ_TREE: UInt8 = 2
private let OBJ_BLOB: UInt8 = 3
private let OBJ_TAG: UInt8 = 4
private let OBJ_OFS_DELTA: UInt8 = 6
private let OBJ_REF_DELTA: UInt8 = 7

/// Result of reading a pack object
public struct PackObject: Sendable {
    public let objType: ObjectType
    public let data: Data
}

public struct IndexedPack: Sendable {
    public let packName: String
    public let packPath: String
    public let indexPath: String
    public let objectCount: Int

    public init(packName: String, packPath: String, indexPath: String, objectCount: Int) {
        self.packName = packName
        self.packPath = packPath
        self.indexPath = indexPath
        self.objectCount = objectCount
    }
}

private struct RawPackEntry {
    let offset: UInt64
    let kind: RawPackEntryKind
}

private enum RawPackEntryKind {
    case base(ObjectType, [UInt8])
    case ofsDelta(baseOffset: UInt64, delta: [UInt8])
    case refDelta(baseOID: OID, delta: [UInt8])
}

private struct ResolvedPackEntry {
    let offset: UInt64
    let oid: OID
    let objType: ObjectType
    let data: [UInt8]
}

/// Read an object from a pack file at the given offset.
public func readPackObject(packPath: String, offset: UInt64, index: PackIndex) throws -> PackObject {
    let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: packPath))
    defer { handle.closeFile() }
    return try readObjectAt(handle: handle, offset: offset, index: index)
}

public func indexPackToODB(gitDir: String, packBytes: [UInt8]) throws -> IndexedPack {
    let (entries, packChecksum) = try parsePackEntries(packBytes)
    let resolved = try resolvePackEntries(entries, gitDir: gitDir)
    let sorted = resolved.sorted { compareRawBytes($0.oid.raw, $1.oid.raw) < 0 }

    let oids = sorted.map { $0.oid }
    let crcs = [UInt32](repeating: 0, count: sorted.count)
    let offsets = sorted.map { $0.offset }
    let idxData = buildPackIndexWithChecksums(
        oids: oids,
        crcs: crcs,
        offsets: offsets,
        packChecksum: packChecksum
    )

    let packDir = (gitDir as NSString).appendingPathComponent("objects/pack")
    try FileManager.default.createDirectory(atPath: packDir, withIntermediateDirectories: true)

    let packHex = hexBytes(packChecksum)
    let packName = "pack-\(packHex)"
    let packPath = (packDir as NSString).appendingPathComponent("\(packName).pack")
    let indexPath = (packDir as NSString).appendingPathComponent("\(packName).idx")

    try writeIfMissing(path: packPath, data: Data(packBytes))
    try writeIfMissing(path: indexPath, data: Data(idxData))

    return IndexedPack(
        packName: packName,
        packPath: packPath,
        indexPath: indexPath,
        objectCount: resolved.count
    )
}

public func buildPackFromOIDs(gitDir: String, roots: [OID], exclude: [OID]) throws -> [UInt8] {
    var visited = Set<OID>()
    let excluded = Set(exclude)
    var ordered: [OID] = []

    for root in roots {
        try collectReachableObjects(
            gitDir: gitDir,
            oid: root,
            exclude: excluded,
            visited: &visited,
            ordered: &ordered
        )
    }

    var buf: [UInt8] = []
    buf.append(contentsOf: Array("PACK".utf8))
    buf.append(contentsOf: packWriteU32(2))
    buf.append(contentsOf: packWriteU32(UInt32(ordered.count)))

    for oid in ordered {
        let obj = try readObject(gitDir: gitDir, oid: oid)
        try appendPackObject(to: &buf, objType: obj.objectType, data: Array(obj.data))
    }

    let checksum = SHA1.hash(buf)
    buf.append(contentsOf: checksum)
    return buf
}

private func readObjectAt(handle: FileHandle, offset: UInt64, index: PackIndex) throws -> PackObject {
    handle.seek(toFileOffset: offset)

    let (typeNum, _) = try readTypeAndSize(handle: handle)

    switch typeNum {
    case OBJ_COMMIT, OBJ_TREE, OBJ_BLOB, OBJ_TAG:
        let objType = try packTypeToObjectType(typeNum)
        let data = try decompressStream(handle: handle)
        return PackObject(objType: objType, data: Data(data))

    case OBJ_OFS_DELTA:
        let baseOffset = try readOfsDeltaOffset(handle: handle)
        let deltaData = try decompressStream(handle: handle)
        let base = try readObjectAt(handle: handle, offset: offset - baseOffset, index: index)
        let result = try applyDelta(base: Array(base.data), delta: deltaData)
        return PackObject(objType: base.objType, data: Data(result))

    case OBJ_REF_DELTA:
        let oidData = handle.readData(ofLength: 20)
        guard oidData.count == 20 else {
            throw MuonGitError.invalidObject("truncated ref delta OID")
        }
        let baseOid = OID(raw: Array(oidData))
        let deltaData = try decompressStream(handle: handle)

        guard let basePackOffset = index.find(baseOid) else {
            throw MuonGitError.notFound("base object \(baseOid.hex) not found in pack index")
        }
        let base = try readObjectAt(handle: handle, offset: basePackOffset, index: index)
        let result = try applyDelta(base: Array(base.data), delta: deltaData)
        return PackObject(objType: base.objType, data: Data(result))

    default:
        throw MuonGitError.invalidObject("unknown pack object type \(typeNum)")
    }
}

private func readTypeAndSize(handle: FileHandle) throws -> (UInt8, UInt64) {
    guard let firstByte = handle.readData(ofLength: 1).first else {
        throw MuonGitError.invalidObject("unexpected EOF in pack")
    }

    let typeNum = (firstByte >> 4) & 0x07
    var size = UInt64(firstByte & 0x0F)
    var shift: UInt64 = 4

    if firstByte & 0x80 != 0 {
        while true {
            guard let c = handle.readData(ofLength: 1).first else {
                throw MuonGitError.invalidObject("unexpected EOF in pack header")
            }
            size |= UInt64(c & 0x7F) << shift
            shift += 7
            if c & 0x80 == 0 { break }
        }
    }

    return (typeNum, size)
}

private func readOfsDeltaOffset(handle: FileHandle) throws -> UInt64 {
    guard let firstByte = handle.readData(ofLength: 1).first else {
        throw MuonGitError.invalidObject("unexpected EOF in ofs delta")
    }
    var c = firstByte
    var offset = UInt64(c & 0x7F)

    while c & 0x80 != 0 {
        offset += 1
        guard let next = handle.readData(ofLength: 1).first else {
            throw MuonGitError.invalidObject("unexpected EOF in ofs delta offset")
        }
        c = next
        offset = (offset << 7) | UInt64(c & 0x7F)
    }

    return offset
}

private func decompressStream(handle: FileHandle) throws -> [UInt8] {
    let currentPos = handle.offsetInFile
    handle.seekToEndOfFile()
    let endPos = handle.offsetInFile
    handle.seek(toFileOffset: currentPos)

    let remaining = Int(endPos - currentPos)
    let compressed = [UInt8](handle.readData(ofLength: remaining))
    let (decompressed, consumed) = try inflateZlibPrefix(compressed)
    handle.seek(toFileOffset: currentPos + UInt64(consumed))
    return decompressed
}

private func packTypeToObjectType(_ t: UInt8) throws -> ObjectType {
    switch t {
    case OBJ_COMMIT: return .commit
    case OBJ_TREE: return .tree
    case OBJ_BLOB: return .blob
    case OBJ_TAG: return .tag
    default: throw MuonGitError.invalidObject("invalid object type \(t)")
    }
}

private func appendPackObject(to buf: inout [UInt8], objType: ObjectType, data: [UInt8]) throws {
    let typeNum: UInt8
    switch objType {
    case .commit: typeNum = OBJ_COMMIT
    case .tree: typeNum = OBJ_TREE
    case .blob: typeNum = OBJ_BLOB
    case .tag: typeNum = OBJ_TAG
    }

    var size = UInt64(data.count)
    var first = (typeNum << 4) | UInt8(size & 0x0F)
    size >>= 4
    if size == 0 {
        buf.append(first)
    } else {
        first |= 0x80
        buf.append(first)
        while size > 0 {
            var byte = UInt8(size & 0x7F)
            size >>= 7
            if size > 0 {
                byte |= 0x80
            }
            buf.append(byte)
        }
    }

    buf.append(contentsOf: try deflateZlibData(data))
}

private func writeIfMissing(path: String, data: Data) throws {
    if FileManager.default.fileExists(atPath: path) {
        return
    }
    try data.write(to: URL(fileURLWithPath: path), options: .atomic)
}

private func hexBytes(_ bytes: [UInt8]) -> String {
    bytes.map { String(format: "%02x", $0) }.joined()
}

private func parsePackEntries(_ data: [UInt8]) throws -> ([RawPackEntry], [UInt8]) {
    guard data.count >= 32 else {
        throw MuonGitError.invalidObject("pack file too short")
    }
    guard Array(data[0..<4]) == Array("PACK".utf8) else {
        throw MuonGitError.invalidObject("bad pack magic")
    }

    let version = packReadU32(data, 4)
    guard version == 2 || version == 3 else {
        throw MuonGitError.invalidObject("unsupported pack version \(version)")
    }

    let objectCount = Int(packReadU32(data, 8))
    let contentLen = data.count - 20
    let expectedChecksum = SHA1.hash(Array(data[0..<contentLen]))
    let packChecksum = Array(data[contentLen..<data.count])
    guard packChecksum == expectedChecksum else {
        throw MuonGitError.invalidObject("pack checksum mismatch")
    }

    var cursor = 12
    var entries: [RawPackEntry] = []
    entries.reserveCapacity(objectCount)

    for _ in 0..<objectCount {
        guard cursor < contentLen else {
            throw MuonGitError.invalidObject("pack truncated before advertised object count")
        }

        let offset = UInt64(cursor)
        let (typeNum, _, headerLen) = try parseTypeAndSize(data, start: cursor, limit: contentLen)
        cursor += headerLen

        let kind: RawPackEntryKind
        switch typeNum {
        case OBJ_COMMIT, OBJ_TREE, OBJ_BLOB, OBJ_TAG:
            let objType = try packTypeToObjectType(typeNum)
            let (inflated, consumed) = try inflateZlibStream(data, start: cursor, end: contentLen)
            cursor += consumed
            kind = .base(objType, inflated)
        case OBJ_OFS_DELTA:
            let (distance, consumedHeader) = try parseOfsDeltaDistance(data, start: cursor, limit: contentLen)
            cursor += consumedHeader
            let (delta, consumed) = try inflateZlibStream(data, start: cursor, end: contentLen)
            cursor += consumed
            guard offset >= distance else {
                throw MuonGitError.invalidObject("invalid ofs-delta base")
            }
            kind = .ofsDelta(baseOffset: offset - distance, delta: delta)
        case OBJ_REF_DELTA:
            guard cursor + 20 <= contentLen else {
                throw MuonGitError.invalidObject("truncated ref-delta base OID")
            }
            let baseOID = OID(raw: Array(data[cursor..<cursor + 20]))
            cursor += 20
            let (delta, consumed) = try inflateZlibStream(data, start: cursor, end: contentLen)
            cursor += consumed
            kind = .refDelta(baseOID: baseOID, delta: delta)
        default:
            throw MuonGitError.invalidObject("unknown pack object type \(typeNum)")
        }

        entries.append(RawPackEntry(offset: offset, kind: kind))
    }

    guard cursor == contentLen else {
        throw MuonGitError.invalidObject("pack contains trailing bytes after object stream")
    }

    return (entries, packChecksum)
}

private func resolvePackEntries(_ entries: [RawPackEntry], gitDir: String?) throws -> [ResolvedPackEntry] {
    var resolved = [ResolvedPackEntry?](repeating: nil, count: entries.count)
    let offsetToIndex = Dictionary(uniqueKeysWithValues: entries.enumerated().map { ($1.offset, $0) })
    var oidToIndex: [OID: Int] = [:]
    var remaining = entries.count

    while remaining > 0 {
        var progressed = false

        for (index, entry) in entries.enumerated() where resolved[index] == nil {
            let resolvedEntry: ResolvedPackEntry?

            switch entry.kind {
            case let .base(objType, data):
                let oid = OID.hash(type: objType, data: data)
                resolvedEntry = ResolvedPackEntry(offset: entry.offset, oid: oid, objType: objType, data: data)

            case let .ofsDelta(baseOffset, delta):
                guard let baseIndex = offsetToIndex[baseOffset], let base = resolved[baseIndex] else {
                    resolvedEntry = nil
                    break
                }
                let data = try applyDelta(base: base.data, delta: delta)
                let oid = OID.hash(type: base.objType, data: data)
                resolvedEntry = ResolvedPackEntry(offset: entry.offset, oid: oid, objType: base.objType, data: data)

            case let .refDelta(baseOID, delta):
                if let baseIndex = oidToIndex[baseOID], let base = resolved[baseIndex] {
                    let data = try applyDelta(base: base.data, delta: delta)
                    let oid = OID.hash(type: base.objType, data: data)
                    resolvedEntry = ResolvedPackEntry(offset: entry.offset, oid: oid, objType: base.objType, data: data)
                } else if let gitDir, let baseObject = try? readObject(gitDir: gitDir, oid: baseOID) {
                    let baseType = baseObject.objectType
                    let baseData = Array(baseObject.data)
                    let data = try applyDelta(base: baseData, delta: delta)
                    let oid = OID.hash(type: baseType, data: data)
                    resolvedEntry = ResolvedPackEntry(offset: entry.offset, oid: oid, objType: baseType, data: data)
                } else {
                    resolvedEntry = nil
                }
            }

            if let resolvedEntry {
                oidToIndex[resolvedEntry.oid] = index
                resolved[index] = resolvedEntry
                remaining -= 1
                progressed = true
            }
        }

        if !progressed {
            throw MuonGitError.invalidObject("could not resolve all pack deltas")
        }
    }

    return resolved.compactMap { $0 }
}

private func parseTypeAndSize(_ data: [UInt8], start: Int, limit: Int) throws -> (UInt8, UInt64, Int) {
    guard start < limit else {
        throw MuonGitError.invalidObject("unexpected EOF in pack object header")
    }
    let first = data[start]
    let typeNum = (first >> 4) & 0x07
    var size = UInt64(first & 0x0F)
    var shift: UInt64 = 4
    var consumed = 1
    var current = first

    while current & 0x80 != 0 {
        guard start + consumed < limit else {
            throw MuonGitError.invalidObject("truncated pack object header")
        }
        current = data[start + consumed]
        size |= UInt64(current & 0x7F) << shift
        shift += 7
        consumed += 1
    }

    return (typeNum, size, consumed)
}

private func parseOfsDeltaDistance(_ data: [UInt8], start: Int, limit: Int) throws -> (UInt64, Int) {
    guard start < limit else {
        throw MuonGitError.invalidObject("unexpected EOF in ofs-delta")
    }

    var consumed = 1
    var c = data[start]
    var offset = UInt64(c & 0x7F)

    while c & 0x80 != 0 {
        guard start + consumed < limit else {
            throw MuonGitError.invalidObject("truncated ofs-delta offset")
        }
        offset += 1
        c = data[start + consumed]
        offset = (offset << 7) | UInt64(c & 0x7F)
        consumed += 1
    }

    return (offset, consumed)
}

private func inflateZlibStream(_ data: [UInt8], start: Int, end: Int) throws -> ([UInt8], Int) {
    try inflateZlibPrefix(Array(data[start..<end]))
}

private func inflateZlibPrefix(_ input: [UInt8]) throws -> ([UInt8], Int) {
    guard !input.isEmpty else {
        throw MuonGitError.invalidObject("empty zlib stream")
    }

    var stream = z_stream()
    let initStatus = inflateInit_(&stream, ZLIB_VERSION, Int32(MemoryLayout<z_stream>.size))
    guard initStatus == Z_OK else {
        throw MuonGitError.invalidObject("failed to initialize zlib stream")
    }
    defer { inflateEnd(&stream) }

    var output: [UInt8] = []
    var dstBuffer = [UInt8](repeating: 0, count: 32 * 1024)
    var inputCopy = input
    var status = Int32(Z_OK)
    let inputCount = inputCopy.count

    try inputCopy.withUnsafeMutableBytes { inputRawBuffer in
        guard let srcBase = inputRawBuffer.baseAddress?.assumingMemoryBound(to: Bytef.self) else {
            throw MuonGitError.invalidObject("empty zlib stream")
        }

        stream.next_in = srcBase
        stream.avail_in = uInt(inputCount)

        while true {
            let dstCount = dstBuffer.count
            try dstBuffer.withUnsafeMutableBytes { dstRawBuffer in
                guard let dstBase = dstRawBuffer.baseAddress?.assumingMemoryBound(to: Bytef.self) else {
                    throw MuonGitError.invalidObject("failed to allocate zlib output buffer")
                }
                stream.next_out = dstBase
                stream.avail_out = uInt(dstCount)
                status = inflate(&stream, Z_NO_FLUSH)
            }

            let produced = dstBuffer.count - Int(stream.avail_out)
            if produced > 0 {
                output.append(contentsOf: dstBuffer[0..<produced])
            }

            if status == Z_STREAM_END {
                break
            }
            if status != Z_OK {
                throw MuonGitError.invalidObject("failed to inflate pack stream")
            }
            if produced == 0 && stream.avail_in == 0 {
                throw MuonGitError.invalidObject("failed to inflate pack stream")
            }
        }
    }

    return (output, inputCount - Int(stream.avail_in))
}

private func deflateZlibData(_ input: [UInt8]) throws -> [UInt8] {
    var stream = z_stream()
    let initStatus = deflateInit_(&stream, Z_DEFAULT_COMPRESSION, ZLIB_VERSION, Int32(MemoryLayout<z_stream>.size))
    guard initStatus == Z_OK else {
        throw MuonGitError.invalidObject("failed to initialize zlib encoder")
    }
    defer { deflateEnd(&stream) }

    var output: [UInt8] = []
    var dstBuffer = [UInt8](repeating: 0, count: 32 * 1024)
    var inputCopy = input
    let inputCount = inputCopy.count
    var status = Int32(Z_OK)

    try inputCopy.withUnsafeMutableBytes { inputRawBuffer in
        let srcBase = inputRawBuffer.baseAddress?.assumingMemoryBound(to: Bytef.self)
        stream.next_in = srcBase
        stream.avail_in = uInt(inputCount)

        while true {
            let dstCount = dstBuffer.count
            try dstBuffer.withUnsafeMutableBytes { outputRawBuffer in
                guard let dstBase = outputRawBuffer.baseAddress?.assumingMemoryBound(to: Bytef.self) else {
                    throw MuonGitError.invalidObject("failed to allocate zlib output buffer")
                }
                stream.next_out = dstBase
                stream.avail_out = uInt(dstCount)
                status = deflate(&stream, Z_FINISH)
            }

            let produced = dstBuffer.count - Int(stream.avail_out)
            if produced > 0 {
                output.append(contentsOf: dstBuffer[0..<produced])
            }

            if status == Z_STREAM_END {
                break
            }
            if status != Z_OK {
                throw MuonGitError.invalidObject("failed to deflate zlib stream")
            }
        }
    }

    return output
}

private func collectReachableObjects(
    gitDir: String,
    oid: OID,
    exclude: Set<OID>,
    visited: inout Set<OID>,
    ordered: inout [OID]
) throws {
    if exclude.contains(oid) || !visited.insert(oid).inserted {
        return
    }

    let obj = try readObject(gitDir: gitDir, oid: oid)
    switch obj.objectType {
    case .commit:
        let commit = try obj.asCommit()
        try collectReachableObjects(gitDir: gitDir, oid: commit.treeId, exclude: exclude, visited: &visited, ordered: &ordered)
        for parent in commit.parentIds {
            try collectReachableObjects(gitDir: gitDir, oid: parent, exclude: exclude, visited: &visited, ordered: &ordered)
        }
    case .tree:
        let tree = try obj.asTree()
        for entry in tree.entries {
            try collectReachableObjects(gitDir: gitDir, oid: entry.oid, exclude: exclude, visited: &visited, ordered: &ordered)
        }
    case .tag:
        let tag = try obj.asTag()
        try collectReachableObjects(gitDir: gitDir, oid: tag.targetId, exclude: exclude, visited: &visited, ordered: &ordered)
    case .blob:
        break
    }

    ordered.append(oid)
}

/// Apply a git delta to a base object.
public func applyDelta(base: [UInt8], delta: [UInt8]) throws -> [UInt8] {
    var pos = 0

    let (_, srcConsumed) = readDeltaSize(delta, pos)
    pos += srcConsumed

    let (tgtSize, tgtConsumed) = readDeltaSize(delta, pos)
    pos += tgtConsumed

    var result: [UInt8] = []
    result.reserveCapacity(Int(tgtSize))

    while pos < delta.count {
        let cmd = delta[pos]
        pos += 1

        if cmd & 0x80 != 0 {
            // Copy from base
            var copyOffset: UInt32 = 0
            var copySize: UInt32 = 0

            if cmd & 0x01 != 0 { copyOffset |= UInt32(delta[pos]); pos += 1 }
            if cmd & 0x02 != 0 { copyOffset |= UInt32(delta[pos]) << 8; pos += 1 }
            if cmd & 0x04 != 0 { copyOffset |= UInt32(delta[pos]) << 16; pos += 1 }
            if cmd & 0x08 != 0 { copyOffset |= UInt32(delta[pos]) << 24; pos += 1 }

            if cmd & 0x10 != 0 { copySize |= UInt32(delta[pos]); pos += 1 }
            if cmd & 0x20 != 0 { copySize |= UInt32(delta[pos]) << 8; pos += 1 }
            if cmd & 0x40 != 0 { copySize |= UInt32(delta[pos]) << 16; pos += 1 }

            if copySize == 0 { copySize = 0x10000 }

            let start = Int(copyOffset)
            let end = start + Int(copySize)
            guard end <= base.count else {
                throw MuonGitError.invalidObject("delta copy out of bounds")
            }
            result.append(contentsOf: base[start..<end])
        } else if cmd > 0 {
            // Insert new data
            let size = Int(cmd)
            guard pos + size <= delta.count else {
                throw MuonGitError.invalidObject("delta insert out of bounds")
            }
            result.append(contentsOf: delta[pos..<pos + size])
            pos += size
        } else {
            throw MuonGitError.invalidObject("invalid delta opcode 0")
        }
    }

    guard result.count == Int(tgtSize) else {
        throw MuonGitError.invalidObject("delta result size mismatch")
    }

    return result
}

private func readDeltaSize(_ data: [UInt8], _ start: Int) -> (UInt64, Int) {
    var pos = start
    var size: UInt64 = 0
    var shift: UInt64 = 0

    while pos < data.count {
        let c = data[pos]
        pos += 1
        size |= UInt64(c & 0x7F) << shift
        shift += 7
        if c & 0x80 == 0 { break }
    }

    return (size, pos - start)
}

/// Build a minimal pack file for testing.
func buildTestPack(objects: [(ObjectType, [UInt8])]) -> [UInt8] {
    var buf: [UInt8] = []

    // Header
    buf.append(contentsOf: Array("PACK".utf8))
    buf.append(contentsOf: packWriteU32(2)) // version
    buf.append(contentsOf: packWriteU32(UInt32(objects.count)))

    for (objType, data) in objects {
        let typeNum: UInt8
        switch objType {
        case .commit: typeNum = OBJ_COMMIT
        case .tree: typeNum = OBJ_TREE
        case .blob: typeNum = OBJ_BLOB
        case .tag: typeNum = OBJ_TAG
        }

        let size = UInt64(data.count)
        var headerBytes: [UInt8] = []
        let first = (typeNum << 4) | UInt8(size & 0x0F)
        var remaining = size >> 4

        if remaining > 0 {
            headerBytes.append(first | 0x80)
            while remaining > 0 {
                let byte = UInt8(remaining & 0x7F)
                remaining >>= 7
                if remaining > 0 {
                    headerBytes.append(byte | 0x80)
                } else {
                    headerBytes.append(byte)
                }
            }
        } else {
            headerBytes.append(first)
        }

        buf.append(contentsOf: headerBytes)

        // Compress data
        buf.append(contentsOf: try! deflateZlibData(data))
    }

    // Pack checksum
    let checksum = SHA1.hash(buf)
    buf.append(contentsOf: checksum)

    return buf
}

private func packWriteU32(_ value: UInt32) -> [UInt8] {
    [UInt8((value >> 24) & 0xFF), UInt8((value >> 16) & 0xFF),
     UInt8((value >> 8) & 0xFF), UInt8(value & 0xFF)]
}

private func packReadU32(_ data: [UInt8], _ offset: Int) -> UInt32 {
    (UInt32(data[offset]) << 24) | (UInt32(data[offset + 1]) << 16) |
    (UInt32(data[offset + 2]) << 8) | UInt32(data[offset + 3])
}

private func compareRawBytes(_ a: [UInt8], _ b: [UInt8]) -> Int {
    for idx in 0..<min(a.count, b.count) {
        if a[idx] < b[idx] { return -1 }
        if a[idx] > b[idx] { return 1 }
    }
    return a.count - b.count
}
