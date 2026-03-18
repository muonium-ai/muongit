/// MuonGit - Blob object read/write
import Foundation

/// A parsed git blob object
public struct Blob: Sendable {
    public let oid: OID
    public let data: Data
    public let size: Int

    public init(oid: OID, data: Data) {
        self.oid = oid
        self.data = data
        self.size = data.count
    }
}

// MARK: - Reading

/// Read a blob from the object database
public func readBlob(gitDir: String, oid: OID) throws -> Blob {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: oid)
    guard objType == .blob else {
        throw MuonGitError.invalidObject("expected blob, got \(objType)")
    }
    return Blob(oid: oid, data: data)
}

// MARK: - Writing

/// Write data as a blob to the object database, returns the OID
@discardableResult
public func writeBlob(gitDir: String, data: Data) throws -> OID {
    return try writeLooseObject(gitDir: gitDir, type: .blob, data: data)
}

/// Write a file's contents as a blob to the object database
@discardableResult
public func writeBlobFromFile(gitDir: String, path: String) throws -> OID {
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    return try writeBlob(gitDir: gitDir, data: data)
}

/// Compute the blob OID for data without writing to the ODB (hash-object --stdin)
public func hashBlob(data: Data) -> OID {
    return OID.hash(type: .blob, data: Array(data))
}
