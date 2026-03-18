/// SHA-1 implementation using CommonCrypto for hardware-accelerated hashing.
/// Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)
import Foundation
import CommonCrypto

public struct SHA1 {
    private var context = CC_SHA1_CTX()

    public init() {
        CC_SHA1_Init(&context)
    }

    public mutating func update(_ data: [UInt8]) {
        data.withUnsafeBufferPointer { ptr in
            CC_SHA1_Update(&context, ptr.baseAddress, CC_LONG(data.count))
        }
    }

    public mutating func update(_ data: Data) {
        data.withUnsafeBytes { ptr in
            CC_SHA1_Update(&context, ptr.baseAddress, CC_LONG(data.count))
        }
    }

    public mutating func update(_ string: String) {
        let bytes = Array(string.utf8)
        update(bytes)
    }

    public mutating func finalize() -> [UInt8] {
        var digest = [UInt8](repeating: 0, count: Int(CC_SHA1_DIGEST_LENGTH))
        CC_SHA1_Final(&digest, &context)
        return digest
    }

    /// Convenience: hash data in one call
    public static func hash(_ data: [UInt8]) -> [UInt8] {
        var digest = [UInt8](repeating: 0, count: Int(CC_SHA1_DIGEST_LENGTH))
        data.withUnsafeBufferPointer { ptr in
            CC_SHA1(ptr.baseAddress, CC_LONG(data.count), &digest)
        }
        return digest
    }

    /// Convenience: hash string
    public static func hash(_ string: String) -> [UInt8] {
        hash(Array(string.utf8))
    }
}

// MARK: - OID SHA-1 Extensions

// Pre-computed type name bytes with trailing space for git object headers
private let typeNameBlob: [UInt8] = [UInt8]("blob ".utf8)
private let typeNameTree: [UInt8] = [UInt8]("tree ".utf8)
private let typeNameCommit: [UInt8] = [UInt8]("commit ".utf8)
private let typeNameTag: [UInt8] = [UInt8]("tag ".utf8)

/// Get pre-computed type name bytes with trailing space
func objectTypeNameBytes(_ type: ObjectType) -> [UInt8] {
    switch type {
    case .blob:   return typeNameBlob
    case .tree:   return typeNameTree
    case .commit: return typeNameCommit
    case .tag:    return typeNameTag
    }
}

/// Build git object header ("type size\0") as byte array without string formatting
func buildObjectHeader(type: ObjectType, size: Int) -> [UInt8] {
    var header = objectTypeNameBytes(type)
    // Append decimal size
    if size == 0 {
        header.append(0x30)
    } else {
        let start = header.count
        var v = size
        while v > 0 {
            header.append(UInt8(v % 10) + 0x30)
            v /= 10
        }
        var lo = start
        var hi = header.count - 1
        while lo < hi {
            let tmp = header[lo]
            header[lo] = header[hi]
            header[hi] = tmp
            lo += 1
            hi -= 1
        }
    }
    header.append(0) // null terminator
    return header
}

extension OID {
    /// Create an OID by hashing data with SHA-1 (git object style)
    public static func hash(type: ObjectType, data: [UInt8]) -> OID {
        let header = buildObjectHeader(type: type, size: data.count)
        var sha = SHA1()
        sha.update(header)
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
