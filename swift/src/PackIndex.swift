/// MuonGit - Pack index (.idx) file parsing
/// Parity: libgit2 src/libgit2/pack.c (index parsing)
import Foundation

private let idxMagic: [UInt8] = [0xFF, 0x74, 0x4F, 0x63] // "\377tOc"
private let idxVersion: UInt32 = 2
private let fanoutCount = 256

/// A parsed pack index file
public struct PackIndex: Sendable {
    public let count: UInt32
    public let fanout: [UInt32]
    public let oids: [OID]
    public let crcs: [UInt32]
    public let offsets: [UInt64]

    /// Look up an OID in the index. Returns the pack file offset if found.
    public func find(_ oid: OID) -> UInt64? {
        let raw = oid.raw
        guard !raw.isEmpty else { return nil }
        let firstByte = Int(raw[0])

        let start = firstByte == 0 ? 0 : Int(fanout[firstByte - 1])
        let end = Int(fanout[firstByte])

        // Binary search within the range
        var lo = start
        var hi = end
        while lo < hi {
            let mid = lo + (hi - lo) / 2
            let cmp = compareBytes(oids[mid].raw, raw)
            if cmp < 0 {
                lo = mid + 1
            } else if cmp > 0 {
                hi = mid
            } else {
                return offsets[mid]
            }
        }
        return nil
    }

    /// Check if the index contains a given OID.
    public func contains(_ oid: OID) -> Bool {
        find(oid) != nil
    }
}

private func compareBytes(_ a: [UInt8], _ b: [UInt8]) -> Int {
    for i in 0..<min(a.count, b.count) {
        if a[i] < b[i] { return -1 }
        if a[i] > b[i] { return 1 }
    }
    return a.count - b.count
}

private func readU32(_ data: [UInt8], _ offset: Int) -> UInt32 {
    (UInt32(data[offset]) << 24) | (UInt32(data[offset+1]) << 16) |
    (UInt32(data[offset+2]) << 8) | UInt32(data[offset+3])
}

/// Parse a pack index file from disk.
public func readPackIndex(path: String) throws -> PackIndex {
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    return try parsePackIndex(Array(data))
}

/// Parse pack index bytes.
public func parsePackIndex(_ data: [UInt8]) throws -> PackIndex {
    guard data.count >= 1072 else {
        throw MuonGitError.invalidObject("pack index too short")
    }

    guard data[0] == 0xFF, data[1] == 0x74, data[2] == 0x4F, data[3] == 0x63 else {
        throw MuonGitError.invalidObject("bad pack index magic")
    }
    let version = readU32(data, 4)
    guard version == 2 else {
        throw MuonGitError.invalidObject("unsupported pack index version \(version)")
    }

    var fanout = [UInt32](repeating: 0, count: fanoutCount)
    for i in 0..<fanoutCount {
        fanout[i] = readU32(data, 8 + i * 4)
    }
    let count = fanout[255]

    let oidTableStart = 8 + fanoutCount * 4
    let crcTableStart = oidTableStart + Int(count) * 20
    let offsetTableStart = crcTableStart + Int(count) * 4
    let minSize = offsetTableStart + Int(count) * 4 + 40
    guard data.count >= minSize else {
        throw MuonGitError.invalidObject("pack index truncated")
    }

    var oids: [OID] = []
    for i in 0..<Int(count) {
        let start = oidTableStart + i * 20
        oids.append(OID(raw: Array(data[start..<start + 20])))
    }

    var crcs: [UInt32] = []
    for i in 0..<Int(count) {
        crcs.append(readU32(data, crcTableStart + i * 4))
    }

    let largeOffsetStart = offsetTableStart + Int(count) * 4
    var offsets: [UInt64] = []
    for i in 0..<Int(count) {
        let rawOffset = readU32(data, offsetTableStart + i * 4)
        if rawOffset & 0x80000000 != 0 {
            let largeIdx = Int(rawOffset & 0x7FFFFFFF)
            let lo = largeOffsetStart + largeIdx * 8
            guard lo + 8 <= data.count else {
                throw MuonGitError.invalidObject("pack index large offset out of bounds")
            }
            var val64: UInt64 = 0
            for j in 0..<8 {
                val64 = (val64 << 8) | UInt64(data[lo + j])
            }
            offsets.append(val64)
        } else {
            offsets.append(UInt64(rawOffset))
        }
    }

    return PackIndex(count: count, fanout: fanout, oids: oids, crcs: crcs, offsets: offsets)
}

/// Build a pack index from components (for testing).
func buildPackIndex(oids: [OID], crcs: [UInt32], offsets: [UInt64]) -> [UInt8] {
    buildPackIndexWithChecksums(oids: oids, crcs: crcs, offsets: offsets, packChecksum: [UInt8](repeating: 0, count: 20))
}

func buildPackIndexWithChecksums(
    oids: [OID],
    crcs: [UInt32],
    offsets: [UInt64],
    packChecksum: [UInt8]
) -> [UInt8] {
    var buf: [UInt8] = []

    buf.append(contentsOf: idxMagic)
    buf.append(contentsOf: writePackU32(idxVersion))

    // Build fanout table
    var fanout = [UInt32](repeating: 0, count: fanoutCount)
    for oid in oids {
        let first = Int(oid.raw[0])
        for j in first..<fanoutCount {
            fanout[j] += 1
        }
    }
    for f in fanout {
        buf.append(contentsOf: writePackU32(f))
    }

    for oid in oids {
        buf.append(contentsOf: oid.raw)
    }

    for crc in crcs {
        buf.append(contentsOf: writePackU32(crc))
    }

    var largeOffsets: [UInt64] = []
    for offset in offsets {
        if offset > 0x7FFF_FFFF {
            let index = UInt32(largeOffsets.count)
            buf.append(contentsOf: writePackU32(0x8000_0000 | index))
            largeOffsets.append(offset)
        } else {
            buf.append(contentsOf: writePackU32(UInt32(offset & 0xFFFFFFFF)))
        }
    }

    for offset in largeOffsets {
        var value = offset
        var encoded = [UInt8](repeating: 0, count: 8)
        for idx in stride(from: 7, through: 0, by: -1) {
            encoded[idx] = UInt8(value & 0xFF)
            value >>= 8
        }
        buf.append(contentsOf: encoded)
    }

    buf.append(contentsOf: packChecksum)

    let checksum = SHA1.hash(buf)
    buf.append(contentsOf: checksum)

    return buf
}

private func writePackU32(_ value: UInt32) -> [UInt8] {
    [UInt8((value >> 24) & 0xFF), UInt8((value >> 16) & 0xFF),
     UInt8((value >> 8) & 0xFF), UInt8(value & 0xFF)]
}
