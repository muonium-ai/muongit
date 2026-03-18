/// Pure Swift SHA-1 implementation
/// Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)
import Foundation

public struct SHA1 {
    private var h0: UInt32 = 0x67452301
    private var h1: UInt32 = 0xEFCDAB89
    private var h2: UInt32 = 0x98BADCFE
    private var h3: UInt32 = 0x10325476
    private var h4: UInt32 = 0xC3D2E1F0

    private var buffer = [UInt8]()
    private var totalLength: UInt64 = 0

    public init() {}

    public mutating func update(_ data: [UInt8]) {
        buffer.append(contentsOf: data)
        totalLength += UInt64(data.count)

        while buffer.count >= 64 {
            let block = Array(buffer.prefix(64))
            processBlock(block)
            buffer.removeFirst(64)
        }
    }

    public mutating func update(_ data: Data) {
        update(Array(data))
    }

    public mutating func update(_ string: String) {
        update(Array(string.utf8))
    }

    public mutating func finalize() -> [UInt8] {
        // Pad message
        var padded = buffer
        padded.append(0x80)

        while padded.count % 64 != 56 {
            padded.append(0x00)
        }

        // Append original length in bits as big-endian 64-bit
        let bitLength = totalLength * 8
        for i in (0..<8).reversed() {
            padded.append(UInt8((bitLength >> (i * 8)) & 0xFF))
        }

        // Process remaining blocks
        var offset = 0
        while offset < padded.count {
            let block = Array(padded[offset..<offset + 64])
            processBlock(block)
            offset += 64
        }

        // Produce digest
        var digest = [UInt8](repeating: 0, count: 20)
        for (i, h) in [h0, h1, h2, h3, h4].enumerated() {
            digest[i * 4]     = UInt8((h >> 24) & 0xFF)
            digest[i * 4 + 1] = UInt8((h >> 16) & 0xFF)
            digest[i * 4 + 2] = UInt8((h >> 8) & 0xFF)
            digest[i * 4 + 3] = UInt8(h & 0xFF)
        }
        return digest
    }

    private mutating func processBlock(_ block: [UInt8]) {
        var w = [UInt32](repeating: 0, count: 80)

        // Load 16 words from block (big-endian)
        for i in 0..<16 {
            w[i] = UInt32(block[i * 4]) << 24
                | UInt32(block[i * 4 + 1]) << 16
                | UInt32(block[i * 4 + 2]) << 8
                | UInt32(block[i * 4 + 3])
        }

        // Extend to 80 words
        for i in 16..<80 {
            w[i] = rotateLeft(w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16], by: 1)
        }

        var a = h0, b = h1, c = h2, d = h3, e = h4

        for i in 0..<80 {
            let f: UInt32
            let k: UInt32

            switch i {
            case 0..<20:
                f = (b & c) | ((~b) & d)
                k = 0x5A827999
            case 20..<40:
                f = b ^ c ^ d
                k = 0x6ED9EBA1
            case 40..<60:
                f = (b & c) | (b & d) | (c & d)
                k = 0x8F1BBCDC
            default:
                f = b ^ c ^ d
                k = 0xCA62C1D6
            }

            let temp = rotateLeft(a, by: 5) &+ f &+ e &+ k &+ w[i]
            e = d
            d = c
            c = rotateLeft(b, by: 30)
            b = a
            a = temp
        }

        h0 = h0 &+ a
        h1 = h1 &+ b
        h2 = h2 &+ c
        h3 = h3 &+ d
        h4 = h4 &+ e
    }

    private func rotateLeft(_ value: UInt32, by count: UInt32) -> UInt32 {
        (value << count) | (value >> (32 - count))
    }

    /// Convenience: hash data in one call
    public static func hash(_ data: [UInt8]) -> [UInt8] {
        var sha = SHA1()
        sha.update(data)
        return sha.finalize()
    }

    /// Convenience: hash string
    public static func hash(_ string: String) -> [UInt8] {
        hash(Array(string.utf8))
    }
}

// MARK: - OID SHA-1 Extensions

extension OID {
    /// Create an OID by hashing data with SHA-1 (git object style)
    public static func hash(type: ObjectType, data: [UInt8]) -> OID {
        let typeName: String
        switch type {
        case .commit: typeName = "commit"
        case .tree:   typeName = "tree"
        case .blob:   typeName = "blob"
        case .tag:    typeName = "tag"
        }

        let header = "\(typeName) \(data.count)\0"
        var sha = SHA1()
        sha.update(Array(header.utf8))
        sha.update(data)
        return OID(raw: sha.finalize())
    }

    /// SHA-1 digest length in bytes
    public static let sha1Length = 20

    /// SHA-1 hex string length
    public static let sha1HexLength = 40

    /// Whether this OID is all zeros
    public var isZero: Bool {
        raw.allSatisfy { $0 == 0 }
    }

    /// Zero OID
    public static let zero = OID(raw: [UInt8](repeating: 0, count: sha1Length))
}
