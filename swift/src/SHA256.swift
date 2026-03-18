/// SHA-256 implementation using CommonCrypto for hardware-accelerated hashing.
/// Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs
import Foundation
import CommonCrypto

public struct SHA256Hash {
    private var context = CC_SHA256_CTX()

    public init() {
        CC_SHA256_Init(&context)
    }

    public mutating func update(_ data: [UInt8]) {
        data.withUnsafeBufferPointer { ptr in
            CC_SHA256_Update(&context, ptr.baseAddress, CC_LONG(data.count))
        }
    }

    public mutating func update(_ string: String) {
        let bytes = Array(string.utf8)
        update(bytes)
    }

    public mutating func finalize() -> [UInt8] {
        var digest = [UInt8](repeating: 0, count: Int(CC_SHA256_DIGEST_LENGTH))
        CC_SHA256_Final(&digest, &context)
        return digest
    }

    /// Convenience: hash data in one call
    public static func hash(_ data: [UInt8]) -> [UInt8] {
        var digest = [UInt8](repeating: 0, count: Int(CC_SHA256_DIGEST_LENGTH))
        data.withUnsafeBufferPointer { ptr in
            CC_SHA256(ptr.baseAddress, CC_LONG(data.count), &digest)
        }
        return digest
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
