/// MuonGit - First-class reference database API
import Foundation

public struct Reference: Sendable, Equatable {
    public let name: String
    public let value: String
    public let symbolicTarget: String?
    public let target: OID?

    public var isSymbolic: Bool { symbolicTarget != nil }

    init(name: String, value: String) {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        self.name = name
        self.value = trimmed
        if trimmed.hasPrefix("ref: ") {
            self.symbolicTarget = String(trimmed.dropFirst(5)).trimmingCharacters(in: .whitespacesAndNewlines)
            self.target = nil
        } else {
            self.symbolicTarget = nil
            self.target = OID(hex: trimmed)
        }
    }
}

public final class RefDb: Sendable {
    public let gitDir: String

    public init(gitDir: String) {
        self.gitDir = gitDir
    }

    public func read(name: String) throws -> Reference {
        Reference(name: name, value: try readReference(gitDir: gitDir, name: name))
    }

    public func resolve(name: String) throws -> OID {
        try resolveReference(gitDir: gitDir, name: name)
    }

    public func list() throws -> [Reference] {
        try listReferences(gitDir: gitDir).map { Reference(name: $0.0, value: $0.1) }
    }

    public func write(name: String, oid: OID) throws {
        try writeReference(gitDir: gitDir, name: name, oid: oid)
    }

    public func writeSymbolic(name: String, target: String) throws {
        try writeSymbolicReference(gitDir: gitDir, name: name, target: target)
    }

    @discardableResult
    public func delete(name: String) throws -> Bool {
        let looseDeleted = try deleteReference(gitDir: gitDir, name: name)
        let packedDeleted = try deletePackedReference(gitDir: gitDir, name: name)
        return looseDeleted || packedDeleted
    }
}

public extension Repository {
    var refdb: RefDb { RefDb(gitDir: gitDir) }
}

func packedReferences(gitDir: String) throws -> [String: String] {
    let packedPath = (gitDir as NSString).appendingPathComponent("packed-refs")
    guard FileManager.default.fileExists(atPath: packedPath) else {
        return [:]
    }

    let content = try String(contentsOfFile: packedPath, encoding: .utf8)
    var refs: [String: String] = [:]
    for line in content.components(separatedBy: .newlines) {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty || trimmed.hasPrefix("#") || trimmed.hasPrefix("^") {
            continue
        }
        let parts = trimmed.split(separator: " ", maxSplits: 1)
        if parts.count == 2 {
            refs[String(parts[1])] = String(parts[0])
        }
    }
    return refs
}

private func deletePackedReference(gitDir: String, name: String) throws -> Bool {
    var refs = try packedReferences(gitDir: gitDir)
    let deleted = refs.removeValue(forKey: name) != nil
    if deleted {
        try writePackedReferences(gitDir: gitDir, refs: refs)
    }
    return deleted
}

private func writePackedReferences(gitDir: String, refs: [String: String]) throws {
    let packedPath = (gitDir as NSString).appendingPathComponent("packed-refs")
    if refs.isEmpty {
        if FileManager.default.fileExists(atPath: packedPath) {
            try FileManager.default.removeItem(atPath: packedPath)
        }
        return
    }

    var lines = ["# pack-refs with: sorted"]
    for name in refs.keys.sorted() {
        lines.append("\(refs[name]!) \(name)")
    }
    try (lines.joined(separator: "\n") + "\n").write(toFile: packedPath, atomically: true, encoding: .utf8)
}
