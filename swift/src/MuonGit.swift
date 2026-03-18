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
        fatalError("TODO: implement clone - requires network transport")
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
    public static let major = 0
    public static let minor = 1
    public static let patch = 0
    public static let string = "\(major).\(minor).\(patch)"
    public static let libgit2Parity = "1.9.0"
}
