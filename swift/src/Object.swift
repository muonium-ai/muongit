/// MuonGit - Generic object lookup and peeling
import Foundation

/// A generic git object loaded from the object database.
public struct GitObject: Sendable, Equatable {
    public let oid: OID
    public let objectType: ObjectType
    public let data: Data
    public var size: Int { data.count }

    public init(oid: OID, objectType: ObjectType, data: Data) {
        self.oid = oid
        self.objectType = objectType
        self.data = data
    }

    public func asBlob() throws -> Blob {
        guard objectType == .blob else {
            throw MuonGitError.invalidObject("expected blob, got \(objectType)")
        }
        return Blob(oid: oid, data: data)
    }

    public func asCommit() throws -> Commit {
        guard objectType == .commit else {
            throw MuonGitError.invalidObject("expected commit, got \(objectType)")
        }
        return try parseCommit(oid: oid, data: data)
    }

    public func asTree() throws -> Tree {
        guard objectType == .tree else {
            throw MuonGitError.invalidObject("expected tree, got \(objectType)")
        }
        return try parseTree(oid: oid, data: data)
    }

    public func asTag() throws -> Tag {
        guard objectType == .tag else {
            throw MuonGitError.invalidObject("expected tag, got \(objectType)")
        }
        return try parseTag(oid: oid, data: data)
    }

    public func peel(gitDir: String) throws -> GitObject {
        var current = self
        var seen: Set<OID> = [oid]

        while current.objectType == .tag {
            let tag = try current.asTag()
            guard seen.insert(tag.targetId).inserted else {
                throw MuonGitError.invalidObject("tag peel cycle detected")
            }
            current = try readObject(gitDir: gitDir, oid: tag.targetId)
        }

        return current
    }
}

/// Read a generic object by OID from loose or packed storage.
public func readObject(gitDir: String, oid: OID) throws -> GitObject {
    do {
        let (objType, data) = try readLooseObject(gitDir: gitDir, oid: oid)
        return GitObject(oid: oid, objectType: objType, data: data)
    } catch MuonGitError.notFound(_) {
        return try readPackedObject(gitDir: gitDir, oid: oid)
    }
}

public extension Repository {
    func readObject(_ oid: OID) throws -> GitObject {
        try MuonGit.readObject(gitDir: gitDir, oid: oid)
    }
}

private func readPackedObject(gitDir: String, oid: OID) throws -> GitObject {
    let packDir = (gitDir as NSString).appendingPathComponent("objects/pack")
    let entries: [String]
    do {
        entries = try FileManager.default.contentsOfDirectory(atPath: packDir)
    } catch {
        throw MuonGitError.notFound("object not found: \(oid.hex)")
    }

    for entry in entries.sorted() where entry.hasSuffix(".idx") {
        let idxPath = (packDir as NSString).appendingPathComponent(entry)
        let idx = try readPackIndex(path: idxPath)
        guard let offset = idx.find(oid) else {
            continue
        }

        let packPath = ((idxPath as NSString).deletingPathExtension as NSString)
            .appendingPathExtension("pack")!
        let packObject = try readPackObject(packPath: packPath, offset: offset, index: idx)
        return GitObject(oid: oid, objectType: packObject.objType, data: packObject.data)
    }

    throw MuonGitError.notFound("object not found: \(oid.hex)")
}
