/// MuonGit - Native Swift port of libgit2
/// API parity target: libgit2 v1.9.0
import Foundation

// MARK: - Core Types

/// Object identifier (SHA-1 / SHA-256)
/// Uses fixed-size inline storage (like libgit2's git_oid) to avoid heap allocation.
public struct OID: Hashable, Sendable {
    // Fixed-size inline storage — 32 bytes max (SHA-256), stack-allocated
    // SHA-1 uses first 20 bytes (a, b, c0); SHA-256 uses all 32 (a, b, c, d)
    private let a: UInt64
    private let b: UInt64
    private let c: UInt64
    private let d: UInt64
    private let size: UInt8  // 20 for SHA-1, 32 for SHA-256

    public var raw: [UInt8] {
        let count = Int(size)
        var result = [UInt8](repeating: 0, count: count)
        withUnsafeBytes(of: a) { src in for i in 0..<min(8, count) { result[i] = src[i] } }
        if count > 8 { withUnsafeBytes(of: b) { src in for i in 0..<min(8, count - 8) { result[8 + i] = src[i] } } }
        if count > 16 { withUnsafeBytes(of: c) { src in for i in 0..<min(8, count - 16) { result[16 + i] = src[i] } } }
        if count > 24 { withUnsafeBytes(of: d) { src in for i in 0..<min(8, count - 24) { result[24 + i] = src[i] } } }
        return result
    }

    public init(raw: [UInt8]) {
        self.size = UInt8(raw.count)
        var la: UInt64 = 0, lb: UInt64 = 0, lc: UInt64 = 0, ld: UInt64 = 0
        raw.withUnsafeBufferPointer { ptr in
            let base = ptr.baseAddress!
            let n = ptr.count
            if n >= 8 { withUnsafeMutableBytes(of: &la) { $0.copyBytes(from: UnsafeRawBufferPointer(start: base, count: 8)) } }
            if n >= 16 { withUnsafeMutableBytes(of: &lb) { $0.copyBytes(from: UnsafeRawBufferPointer(start: base + 8, count: 8)) } }
            if n >= 24 { withUnsafeMutableBytes(of: &lc) { $0.copyBytes(from: UnsafeRawBufferPointer(start: base + 16, count: min(8, n - 16))) } }
            if n >= 32 { withUnsafeMutableBytes(of: &ld) { $0.copyBytes(from: UnsafeRawBufferPointer(start: base + 24, count: 8)) } }
            // Handle 20-byte SHA-1: copy remaining 4 bytes into lc
            if n >= 20 && n < 24 {
                withUnsafeMutableBytes(of: &lc) { $0.copyBytes(from: UnsafeRawBufferPointer(start: base + 16, count: n - 16)) }
            }
        }
        self.a = la; self.b = lb; self.c = lc; self.d = ld
    }

    public init(hex: String) {
        let bytes: [UInt8] = stride(from: 0, to: hex.count, by: 2).compactMap {
            let start = hex.index(hex.startIndex, offsetBy: $0)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        self.init(raw: bytes)
    }

    init(data: Data) {
        self.init(raw: Array(data))
    }

    public var hex: String {
        raw.map { String(format: "%02x", $0) }.joined()
    }

    /// Initialize from a raw pointer without copying to an intermediate array
    init(unsafeRawPointer ptr: UnsafePointer<UInt8>, count: Int) {
        self.size = UInt8(count)
        var la: UInt64 = 0, lb: UInt64 = 0, lc: UInt64 = 0, ld: UInt64 = 0
        if count >= 8 { withUnsafeMutableBytes(of: &la) { $0.copyBytes(from: UnsafeRawBufferPointer(start: ptr, count: 8)) } }
        if count >= 16 { withUnsafeMutableBytes(of: &lb) { $0.copyBytes(from: UnsafeRawBufferPointer(start: ptr + 8, count: 8)) } }
        if count >= 20 && count < 24 {
            withUnsafeMutableBytes(of: &lc) { $0.copyBytes(from: UnsafeRawBufferPointer(start: ptr + 16, count: count - 16)) }
        }
        if count >= 24 { withUnsafeMutableBytes(of: &lc) { $0.copyBytes(from: UnsafeRawBufferPointer(start: ptr + 16, count: 8)) } }
        if count >= 32 { withUnsafeMutableBytes(of: &ld) { $0.copyBytes(from: UnsafeRawBufferPointer(start: ptr + 24, count: 8)) } }
        self.a = la; self.b = lb; self.c = lc; self.d = ld
    }

    /// Append raw OID bytes directly to a Data buffer (avoids intermediate array)
    func appendRawBytes(to data: inout Data) {
        var copy = self
        withUnsafeBytes(of: &copy) { ptr in
            data.append(ptr.baseAddress!.assumingMemoryBound(to: UInt8.self), count: Int(size))
        }
    }

    private static let hexLookup: [UInt8] = Array("0123456789abcdef".utf8)

    /// Append hex representation of this OID directly to a Data buffer (avoids intermediate String)
    func appendHexBytes(to data: inout Data) {
        var copy = self
        let count = Int(size)
        withUnsafeBytes(of: &copy) { ptr in
            let bytes = ptr.baseAddress!.assumingMemoryBound(to: UInt8.self)
            for i in 0..<count {
                let byte = bytes[i]
                data.append(OID.hexLookup[Int(byte >> 4)])
                data.append(OID.hexLookup[Int(byte & 0x0F)])
            }
        }
    }

    /// Append hex representation of this OID directly to a UInt8 buffer (fastest path)
    func appendHexBytes(to buf: inout [UInt8]) {
        var copy = self
        let count = Int(size)
        withUnsafeBytes(of: &copy) { ptr in
            let bytes = ptr.baseAddress!.assumingMemoryBound(to: UInt8.self)
            for i in 0..<count {
                let byte = bytes[i]
                buf.append(OID.hexLookup[Int(byte >> 4)])
                buf.append(OID.hexLookup[Int(byte & 0x0F)])
            }
        }
    }

    public static func == (lhs: OID, rhs: OID) -> Bool {
        lhs.a == rhs.a && lhs.b == rhs.b && lhs.c == rhs.c && lhs.d == rhs.d
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(a)
        hasher.combine(b)
        hasher.combine(c)
        hasher.combine(d)
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

// MARK: - Repository

/// A Git repository
public final class Repository: Sendable {
    /// Path to the .git directory
    public let gitDir: String
    /// Path to the working directory (nil for bare repos)
    public let workdir: String?
    /// Whether this is a bare repository
    public let isBare: Bool

    private init(gitDir: String, workdir: String?, isBare: Bool) {
        self.gitDir = gitDir
        self.workdir = workdir
        self.isBare = isBare
    }

    /// Open an existing repository at the given path
    public static func open(at path: String) throws -> Repository {
        // Check if path itself is a bare repo
        if isGitDir(path) {
            return Repository(gitDir: path, workdir: nil, isBare: true)
        }

        // Check for .git directory
        let gitDir = (path as NSString).appendingPathComponent(".git")
        if isGitDir(gitDir) {
            return Repository(gitDir: gitDir, workdir: path, isBare: false)
        }

        throw MuonGitError.notFound("could not find repository at '\(path)'")
    }

    /// Discover a repository by walking up from the given path
    public static func discover(at path: String) throws -> Repository {
        var current = path
        while true {
            if let repo = try? open(at: current) {
                return repo
            }
            let parent = (current as NSString).deletingLastPathComponent
            if parent == current { break }
            current = parent
        }
        throw MuonGitError.notFound("could not find repository in any parent directory")
    }

    /// Initialize a new repository
    public static func create(at path: String, bare: Bool = false) throws -> Repository {
        let gitDir = bare ? path : (path as NSString).appendingPathComponent(".git")

        try FileManager.default.createDirectory(atPath: gitDir, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(atPath: (gitDir as NSString).appendingPathComponent("objects"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(atPath: ((gitDir as NSString).appendingPathComponent("refs") as NSString).appendingPathComponent("heads"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(atPath: ((gitDir as NSString).appendingPathComponent("refs") as NSString).appendingPathComponent("tags"), withIntermediateDirectories: true)

        // Write HEAD
        try "ref: refs/heads/main\n".write(
            toFile: (gitDir as NSString).appendingPathComponent("HEAD"),
            atomically: true, encoding: .utf8
        )

        // Write config
        let config = bare
            ? "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n"
            : "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n"
        try config.write(
            toFile: (gitDir as NSString).appendingPathComponent("config"),
            atomically: true, encoding: .utf8
        )

        return Repository(gitDir: gitDir, workdir: bare ? nil : path, isBare: bare)
    }

    /// Clone a repository from a URL
    public static func clone(from url: String, to path: String) throws -> Repository {
        try cloneRepository(from: url, to: path)
    }

    /// Clone a repository from a URL using explicit clone options.
    public static func clone(from url: String, to path: String, options: CloneOptions) throws -> Repository {
        try cloneRepository(from: url, to: path, options: options)
    }

    /// Read HEAD reference
    public func head() throws -> String {
        let headPath = (gitDir as NSString).appendingPathComponent("HEAD")
        let content = try String(contentsOfFile: headPath, encoding: .utf8)
        return content.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Check if HEAD is unborn
    public var isHeadUnborn: Bool {
        guard let headContent = try? head() else { return true }
        if headContent.hasPrefix("ref: ") {
            let refPath = (gitDir as NSString).appendingPathComponent(
                String(headContent.dropFirst(5))
            )
            return !FileManager.default.fileExists(atPath: refPath)
        }
        return false
    }

    /// Check if a directory looks like a .git directory
    private static func isGitDir(_ path: String) -> Bool {
        let fm = FileManager.default
        var isDir: ObjCBool = false
        let hasHEAD = fm.fileExists(atPath: (path as NSString).appendingPathComponent("HEAD"))
        let hasObjects = fm.fileExists(atPath: (path as NSString).appendingPathComponent("objects"), isDirectory: &isDir) && isDir.boolValue
        isDir = false
        let hasRefs = fm.fileExists(atPath: (path as NSString).appendingPathComponent("refs"), isDirectory: &isDir) && isDir.boolValue
        return hasHEAD && hasObjects && hasRefs
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
    public static let major = GeneratedVersion.major
    public static let minor = GeneratedVersion.minor
    public static let patch = GeneratedVersion.patch
    public static let string = GeneratedVersion.string
    public static let libgit2Parity = "1.9.0"
}
