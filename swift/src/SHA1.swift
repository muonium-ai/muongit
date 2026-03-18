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
