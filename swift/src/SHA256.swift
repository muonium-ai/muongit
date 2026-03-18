/// Pure Swift SHA-256 implementation
/// Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs
import Foundation

public struct SHA256Hash {
    private var h0: UInt32 = 0x6a09e667
    private var h1: UInt32 = 0xbb67ae85
    private var h2: UInt32 = 0x3c6ef372
    private var h3: UInt32 = 0xa54ff53a
    private var h4: UInt32 = 0x510e527f
    private var h5: UInt32 = 0x9b05688c
    private var h6: UInt32 = 0x1f83d9ab
    private var h7: UInt32 = 0x5be0cd19

    private var buffer = [UInt8]()
    private var totalLength: UInt64 = 0

    private static let k: [UInt32] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ]

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

    public mutating func update(_ string: String) {
        update(Array(string.utf8))
    }

    public mutating func finalize() -> [UInt8] {
        var padded = buffer
        padded.append(0x80)

        while padded.count % 64 != 56 {
            padded.append(0x00)
        }

        let bitLength = totalLength * 8
        for i in (0..<8).reversed() {
            padded.append(UInt8((bitLength >> (i * 8)) & 0xFF))
        }

        var offset = 0
        while offset < padded.count {
            let block = Array(padded[offset..<offset + 64])
            processBlock(block)
            offset += 64
        }

        var digest = [UInt8](repeating: 0, count: 32)
        for (i, h) in [h0, h1, h2, h3, h4, h5, h6, h7].enumerated() {
            digest[i * 4]     = UInt8((h >> 24) & 0xFF)
            digest[i * 4 + 1] = UInt8((h >> 16) & 0xFF)
            digest[i * 4 + 2] = UInt8((h >> 8) & 0xFF)
            digest[i * 4 + 3] = UInt8(h & 0xFF)
        }
        return digest
    }

    private mutating func processBlock(_ block: [UInt8]) {
        var w = [UInt32](repeating: 0, count: 64)

        for i in 0..<16 {
            w[i] = UInt32(block[i * 4]) << 24
                | UInt32(block[i * 4 + 1]) << 16
                | UInt32(block[i * 4 + 2]) << 8
                | UInt32(block[i * 4 + 3])
        }

        for i in 16..<64 {
            let s0 = rotateRight(w[i - 15], by: 7) ^ rotateRight(w[i - 15], by: 18) ^ (w[i - 15] >> 3)
            let s1 = rotateRight(w[i - 2], by: 17) ^ rotateRight(w[i - 2], by: 19) ^ (w[i - 2] >> 10)
            w[i] = w[i - 16] &+ s0 &+ w[i - 7] &+ s1
        }

        var a = h0, b = h1, c = h2, d = h3
        var e = h4, f = h5, g = h6, h = h7

        for i in 0..<64 {
            let s1 = rotateRight(e, by: 6) ^ rotateRight(e, by: 11) ^ rotateRight(e, by: 25)
            let ch = (e & f) ^ ((~e) & g)
            let temp1 = h &+ s1 &+ ch &+ Self.k[i] &+ w[i]
            let s0 = rotateRight(a, by: 2) ^ rotateRight(a, by: 13) ^ rotateRight(a, by: 22)
            let maj = (a & b) ^ (a & c) ^ (b & c)
            let temp2 = s0 &+ maj

            h = g
            g = f
            f = e
            e = d &+ temp1
            d = c
            c = b
            b = a
            a = temp1 &+ temp2
        }

        h0 = h0 &+ a
        h1 = h1 &+ b
        h2 = h2 &+ c
        h3 = h3 &+ d
        h4 = h4 &+ e
        h5 = h5 &+ f
        h6 = h6 &+ g
        h7 = h7 &+ h
    }

    private func rotateRight(_ value: UInt32, by count: UInt32) -> UInt32 {
        (value >> count) | (value << (32 - count))
    }

    /// Convenience: hash data in one call
    public static func hash(_ data: [UInt8]) -> [UInt8] {
        var sha = SHA256Hash()
        sha.update(data)
        return sha.finalize()
    }

    /// Convenience: hash string
    public static func hash(_ string: String) -> [UInt8] {
        hash(Array(string.utf8))
    }
}

// MARK: - Hash Algorithm

/// Hash algorithm selection (matching libgit2 EXPERIMENTAL_SHA256)
public enum HashAlgorithm: Sendable {
    case sha1
    case sha256

    /// Digest length in bytes
    public var digestLength: Int {
        switch self {
        case .sha1: return 20
        case .sha256: return 32
        }
    }

    /// Hex string length
    public var hexLength: Int {
        digestLength * 2
    }
}

// MARK: - OID SHA-256 Extensions

extension OID {
    /// SHA-256 digest length in bytes
    public static let sha256Length = 32

    /// SHA-256 hex string length
    public static let sha256HexLength = 64

    /// Create an OID by hashing data with SHA-256 (git object style, experimental)
    public static func hashSHA256(type: ObjectType, data: [UInt8]) -> OID {
        let typeName: String
        switch type {
        case .commit: typeName = "commit"
        case .tree:   typeName = "tree"
        case .blob:   typeName = "blob"
        case .tag:    typeName = "tag"
        }

        let header = "\(typeName) \(data.count)\0"
        var sha = SHA256Hash()
        sha.update(Array(header.utf8))
        sha.update(data)
        return OID(raw: sha.finalize())
    }

    /// Zero OID for SHA-256
    public static let zeroSHA256 = OID(raw: [UInt8](repeating: 0, count: sha256Length))
}
