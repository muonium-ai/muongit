/// MuonGit - Git grafts: commit parent overrides
/// Parity: libgit2 src/libgit2/grafts.c
import Foundation

/// A graft entry: a commit with overridden parents
public struct Graft: Sendable {
    public let oid: OID
    public let parents: [OID]
}

/// A collection of grafts loaded from .git/info/grafts or .git/shallow
public struct Grafts: Sendable {
    private var entries: [String: Graft] = [:]

    public init() {}

    /// Load grafts from a file
    public static func load(path: String) throws -> Grafts {
        var grafts = Grafts()
        let fm = FileManager.default
        if fm.fileExists(atPath: path) {
            let content = try String(contentsOfFile: path, encoding: .utf8)
            try grafts.parse(content)
        }
        return grafts
    }

    /// Load grafts for a repository
    public static func loadForRepo(gitDir: String) throws -> Grafts {
        let path = (gitDir as NSString).appendingPathComponent("info/grafts")
        return try load(path: path)
    }

    /// Load shallow entries
    public static func loadShallow(gitDir: String) throws -> Grafts {
        let path = (gitDir as NSString).appendingPathComponent("shallow")
        let fm = FileManager.default
        guard fm.fileExists(atPath: path) else { return Grafts() }

        let content = try String(contentsOfFile: path, encoding: .utf8)
        var grafts = Grafts()
        for line in content.components(separatedBy: "\n") {
            let line = line.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            let oid = OID(hex: line)
            grafts.add(Graft(oid: oid, parents: []))
        }
        return grafts
    }

    /// Parse grafts from content string
    public mutating func parse(_ content: String) throws {
        for line in content.components(separatedBy: "\n") {
            let line = line.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }

            let parts = line.components(separatedBy: " ").filter { !$0.isEmpty }
            guard !parts.isEmpty else { continue }

            let oid = OID(hex: parts[0])
            var parents: [OID] = []
            for i in 1..<parts.count {
                let parentOid = OID(hex: parts[i])
                parents.append(parentOid)
            }
            add(Graft(oid: oid, parents: parents))
        }
    }

    /// Add a graft entry
    public mutating func add(_ graft: Graft) {
        entries[graft.oid.hex] = graft
    }

    /// Remove a graft entry
    @discardableResult
    public mutating func remove(_ oid: OID) -> Bool {
        entries.removeValue(forKey: oid.hex) != nil
    }

    /// Look up a graft for a commit
    public func get(_ oid: OID) -> Graft? {
        entries[oid.hex]
    }

    /// Check if a commit has a graft
    public func contains(_ oid: OID) -> Bool {
        entries[oid.hex] != nil
    }

    /// Get parents for a commit, returning grafted parents if available
    public func getParents(_ oid: OID) -> [OID]? {
        entries[oid.hex]?.parents
    }

    /// Number of graft entries
    public var count: Int { entries.count }

    /// Whether the grafts set is empty
    public var isEmpty: Bool { entries.isEmpty }

    /// List all grafted commit OIDs
    public var oids: [OID] { entries.values.map { $0.oid } }
}
