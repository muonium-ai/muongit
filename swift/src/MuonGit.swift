/// MuonGit - Native Swift port of libgit2
/// API parity target: libgit2 v1.9.0
import Foundation

// MARK: - Core Types

/// Object identifier (SHA-1 / SHA-256)
public struct OID: Hashable, Sendable {
    public let raw: [UInt8]

    public init(raw: [UInt8]) {
        self.raw = raw
    }

    public init(hex: String) {
        self.raw = stride(from: 0, to: hex.count, by: 2).compactMap {
            let start = hex.index(hex.startIndex, offsetBy: $0)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
    }

    public var hex: String {
        raw.map { String(format: "%02x", $0) }.joined()
    }
}

/// Git object types
public enum ObjectType: Int, Sendable {
    case commit = 1
    case tree   = 2
    case blob   = 3
    case tag    = 4
}

/// Git signature (author/committer)
public struct Signature: Sendable {
    public let name: String
    public let email: String
    public let time: Int64
    public let offset: Int32

    public init(name: String, email: String, time: Int64 = 0, offset: Int32 = 0) {
        self.name = name
        self.email = email
        self.time = time
        self.offset = offset
    }
}

// MARK: - Error Handling

/// Errors from MuonGit operations
public enum MuonGitError: Error, Sendable {
    case notFound(String)
    case invalidObject(String)
    case ambiguous(String)
    case bufferTooShort
    case user(String)
    case bareRepo
    case unbornBranch
    case unmerged
    case notFastForward
    case invalidSpec(String)
    case conflict(String)
    case locked(String)
    case modified(String)
    case auth(String)
    case certificate(String)
    case applied
    case peel(String)
    case eof
    case invalid(String)
    case uncommitted(String)
    case directory(String)
    case mergeConflict(String)
}

// MARK: - Version

/// Library version information
public enum MuonGitVersion {
    public static let major = 0
    public static let minor = 1
    public static let patch = 0
    public static let string = "\(major).\(minor).\(patch)"
    public static let libgit2Parity = "1.9.0"
}
