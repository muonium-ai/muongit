/// MuonGit - Tag object read/write
import Foundation

/// A parsed git annotated tag object
public struct Tag: Sendable {
    public let oid: OID
    public let targetId: OID
    public let targetType: ObjectType
    public let tagName: String
    public let tagger: Signature?
    public let message: String
}

// MARK: - Parsing

/// Parse a tag object from its raw data content
public func parseTag(oid: OID, data: Data) throws -> Tag {
    guard let text = String(data: data, encoding: .utf8) else {
        throw MuonGitError.invalidObject("tag is not valid UTF-8")
    }

    var targetId: OID?
    var targetType: ObjectType?
    var tagName: String?
    var tagger: Signature?

    let parts = text.split(separator: "\n\n", maxSplits: 1)
    let headerSection = String(parts[0])
    let message = parts.count > 1 ? String(parts[1]) : ""

    for line in headerSection.split(separator: "\n", omittingEmptySubsequences: false) {
        let line = String(line)
        if line.hasPrefix("object ") {
            targetId = OID(hex: String(line.dropFirst(7)))
        } else if line.hasPrefix("type ") {
            targetType = parseObjectTypeName(String(line.dropFirst(5)))
        } else if line.hasPrefix("tag ") {
            tagName = String(line.dropFirst(4))
        } else if line.hasPrefix("tagger ") {
            tagger = parseSignature(String(line.dropFirst(7)))
        }
    }

    guard let target = targetId else {
        throw MuonGitError.invalidObject("tag missing object")
    }
    guard let type = targetType else {
        throw MuonGitError.invalidObject("tag missing type")
    }
    guard let name = tagName else {
        throw MuonGitError.invalidObject("tag missing tag name")
    }

    return Tag(oid: oid, targetId: target, targetType: type, tagName: name, tagger: tagger, message: message)
}

// MARK: - Serialization

/// Serialize a tag to its raw data representation (without the object header)
public func serializeTag(
    targetId: OID,
    targetType: ObjectType,
    tagName: String,
    tagger: Signature?,
    message: String
) -> Data {
    var lines: [String] = []
    lines.append("object \(targetId.hex)")
    lines.append("type \(objectTypeName(targetType))")
    lines.append("tag \(tagName)")
    if let tagger = tagger {
        lines.append("tagger \(formatSignature(tagger))")
    }
    let header = lines.joined(separator: "\n")
    let raw = "\(header)\n\n\(message)"
    return Data(raw.utf8)
}

// MARK: - Helpers

// objectTypeName is defined in ODB.swift

func parseObjectTypeName(_ name: String) -> ObjectType? {
    return try? objectTypeFromName(name)
}
