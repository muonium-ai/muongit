/// Loose object read/write for the git object database
/// Parity: libgit2 src/libgit2/odb_loose.c
import Foundation

// MARK: - Object Type String Helpers

func objectTypeName(_ type: ObjectType) -> String {
    switch type {
    case .commit: return "commit"
    case .tree:   return "tree"
    case .blob:   return "blob"
    case .tag:    return "tag"
    }
}

func objectTypeFromName(_ name: String) throws -> ObjectType {
    switch name {
    case "commit": return .commit
    case "tree":   return .tree
    case "blob":   return .blob
    case "tag":    return .tag
    default: throw MuonGitError.invalidObject("unknown object type '\(name)'")
    }
}

// MARK: - Loose Object Read

/// Read a loose object from the git object database.
/// - Parameters:
///   - gitDir: Path to the .git directory
///   - oid: The object identifier to read
/// - Returns: A tuple of (ObjectType, Data) with the object's type and content
public func readLooseObject(gitDir: String, oid: OID) throws -> (ObjectType, Data) {
    let hex = oid.hex
    guard hex.count == OID.sha1HexLength else {
        throw MuonGitError.invalidObject("invalid OID hex length")
    }

    let prefix = String(hex.prefix(2))
    let suffix = String(hex.dropFirst(2))
    let objectPath = (gitDir as NSString)
        .appendingPathComponent("objects")
        .appending("/\(prefix)/\(suffix)")

    guard FileManager.default.fileExists(atPath: objectPath) else {
        throw MuonGitError.notFound("loose object not found: \(hex)")
    }

    let compressedData = try Data(contentsOf: URL(fileURLWithPath: objectPath))

    // Decompress using zlib via NSData
    let decompressed = try (compressedData as NSData).decompressed(using: .zlib) as Data

    // Parse header: "{type} {size}\0{content}"
    guard let nullIndex = decompressed.firstIndex(of: 0) else {
        throw MuonGitError.invalidObject("missing null byte in object header")
    }

    let headerData = decompressed[decompressed.startIndex..<nullIndex]
    guard let header = String(data: headerData, encoding: .utf8) else {
        throw MuonGitError.invalidObject("invalid object header encoding")
    }

    let parts = header.split(separator: " ", maxSplits: 1)
    guard parts.count == 2 else {
        throw MuonGitError.invalidObject("malformed object header: '\(header)'")
    }

    let typeName = String(parts[0])
    let type = try objectTypeFromName(typeName)

    guard let declaredSize = Int(parts[1]) else {
        throw MuonGitError.invalidObject("invalid size in object header")
    }

    let content = decompressed[(nullIndex + 1)...]
    guard content.count == declaredSize else {
        throw MuonGitError.invalidObject("object size mismatch: declared \(declaredSize), actual \(content.count)")
    }

    return (type, Data(content))
}

// MARK: - Loose Object Write

/// Write a loose object to the git object database.
/// - Parameters:
///   - gitDir: Path to the .git directory
///   - type: The object type (blob, tree, commit, tag)
///   - data: The raw object content
/// - Returns: The OID of the written object
@discardableResult
public func writeLooseObject(gitDir: String, type: ObjectType, data: Data) throws -> OID {
    // Compute SHA-1 hash of "{type} {size}\0{data}"
    let oid = OID.hash(type: type, data: Array(data))
    let hex = oid.hex

    let prefix = String(hex.prefix(2))
    let suffix = String(hex.dropFirst(2))

    let objectDir = (gitDir as NSString)
        .appendingPathComponent("objects")
        .appending("/\(prefix)")
    let objectPath = objectDir.appending("/\(suffix)")

    // If object already exists, skip writing
    if FileManager.default.fileExists(atPath: objectPath) {
        return oid
    }

    // Build raw object: header + content
    let header = "\(objectTypeName(type)) \(data.count)\0"
    var rawData = Data(header.utf8)
    rawData.append(data)

    // Compress with zlib
    let compressed = try (rawData as NSData).compressed(using: .zlib) as Data

    // Ensure directory exists
    try FileManager.default.createDirectory(atPath: objectDir, withIntermediateDirectories: true)

    // Write atomically
    try compressed.write(to: URL(fileURLWithPath: objectPath), options: .atomic)

    return oid
}
