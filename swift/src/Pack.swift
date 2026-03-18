/// MuonGit - Pack file object lookup and delta resolution
/// Parity: libgit2 src/libgit2/pack.c
import Foundation

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

/// Read an object from a pack file at the given offset.
public func readPackObject(packPath: String, offset: UInt64, index: PackIndex) throws -> PackObject {
    let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: packPath))
    defer { handle.closeFile() }
    return try readObjectAt(handle: handle, offset: offset, index: index)
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
    let compressed = handle.readData(ofLength: remaining)

    let decompressed = try (compressed as NSData).decompressed(using: .zlib) as Data
    return Array(decompressed)
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
        let compressed = try! (Data(data) as NSData).compressed(using: .zlib) as Data
        buf.append(contentsOf: Array(compressed))
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
