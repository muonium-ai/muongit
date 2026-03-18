/// MuonGit - Commit object read/write
import Foundation

/// A parsed git commit object
public struct Commit: Sendable {
    public let oid: OID
    public let treeId: OID
    public let parentIds: [OID]
    public let author: Signature
    public let committer: Signature
    public let message: String
    public let messageEncoding: String?
}

// MARK: - Parsing

/// Parse a commit object from its raw data content
public func parseCommit(oid: OID, data: Data) throws -> Commit {
    guard let text = String(data: data, encoding: .utf8) else {
        throw MuonGitError.invalidObject("commit is not valid UTF-8")
    }

    var treeId: OID?
    var parentIds: [OID] = []
    var author: Signature?
    var committer: Signature?
    var messageEncoding: String?

    // Split into header and message at first blank line
    let parts = text.split(separator: "\n\n", maxSplits: 1)
    let headerSection = String(parts[0])
    let message = parts.count > 1 ? String(parts[1]) : ""

    for line in headerSection.split(separator: "\n", omittingEmptySubsequences: false) {
        let line = String(line)
        if line.hasPrefix("tree ") {
            let hex = String(line.dropFirst(5))
            treeId = OID(hex: hex)
        } else if line.hasPrefix("parent ") {
            let hex = String(line.dropFirst(7))
            parentIds.append(OID(hex: hex))
        } else if line.hasPrefix("author ") {
            author = parseSignature(String(line.dropFirst(7)))
        } else if line.hasPrefix("committer ") {
            committer = parseSignature(String(line.dropFirst(10)))
        } else if line.hasPrefix("encoding ") {
            messageEncoding = String(line.dropFirst(9))
        }
    }

    guard let tree = treeId else {
        throw MuonGitError.invalidObject("commit missing tree")
    }
    guard let auth = author else {
        throw MuonGitError.invalidObject("commit missing author")
    }
    guard let comm = committer else {
        throw MuonGitError.invalidObject("commit missing committer")
    }

    return Commit(
        oid: oid,
        treeId: tree,
        parentIds: parentIds,
        author: auth,
        committer: comm,
        message: message,
        messageEncoding: messageEncoding
    )
}

// MARK: - Serialization

/// Serialize a commit to its raw data representation (without the object header)
public func serializeCommit(
    treeId: OID,
    parentIds: [OID],
    author: Signature,
    committer: Signature,
    message: String,
    messageEncoding: String? = nil
) -> Data {
    var lines: [String] = []
    lines.append("tree \(treeId.hex)")
    for pid in parentIds {
        lines.append("parent \(pid.hex)")
    }
    lines.append("author \(formatSignature(author))")
    lines.append("committer \(formatSignature(committer))")
    if let enc = messageEncoding {
        lines.append("encoding \(enc)")
    }
    let header = lines.joined(separator: "\n")
    let raw = "\(header)\n\n\(message)"
    return Data(raw.utf8)
}

// MARK: - Signature helpers

/// Parse "Name <email> timestamp offset" into a Signature
func parseSignature(_ s: String) -> Signature {
    // Format: "Name <email> 1234567890 +0000"
    guard let emailStart = s.firstIndex(of: "<"),
          let emailEnd = s.firstIndex(of: ">") else {
        return Signature(name: s, email: "")
    }

    let name = String(s[s.startIndex..<emailStart]).trimmingCharacters(in: .whitespaces)
    let email = String(s[s.index(after: emailStart)..<emailEnd])

    let remainder = String(s[s.index(after: emailEnd)...]).trimmingCharacters(in: .whitespaces)
    let parts = remainder.split(separator: " ")

    var time: Int64 = 0
    var offset: Int32 = 0
    if parts.count >= 1, let t = Int64(parts[0]) {
        time = t
    }
    if parts.count >= 2 {
        offset = parseTimezoneOffset(String(parts[1]))
    }

    return Signature(name: name, email: email, time: time, offset: offset)
}

/// Format a Signature into "Name <email> timestamp offset"
func formatSignature(_ sig: Signature) -> String {
    let sign = sig.offset >= 0 ? "+" : "-"
    let absOffset = abs(sig.offset)
    let hours = absOffset / 60
    let minutes = absOffset % 60
    return "\(sig.name) <\(sig.email)> \(sig.time) \(sign)\(String(format: "%02d%02d", hours, minutes))"
}

/// Parse "+0530" or "-0800" into minutes offset
func parseTimezoneOffset(_ s: String) -> Int32 {
    guard s.count >= 5 else { return 0 }
    let sign: Int32 = s.hasPrefix("-") ? -1 : 1
    let digits = String(s.dropFirst())
    guard digits.count == 4,
          let hours = Int32(digits.prefix(2)),
          let minutes = Int32(digits.suffix(2)) else { return 0 }
    return sign * (hours * 60 + minutes)
}
