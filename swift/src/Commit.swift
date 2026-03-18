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

// Pre-computed byte arrays for commit serialization prefixes
private let commitTreePrefix: [UInt8] = [UInt8]("tree ".utf8)
private let commitParentPrefix: [UInt8] = [UInt8]("parent ".utf8)
private let commitAuthorPrefix: [UInt8] = [UInt8]("author ".utf8)
private let commitCommitterPrefix: [UInt8] = [UInt8]("committer ".utf8)
private let commitEncodingPrefix: [UInt8] = [UInt8]("encoding ".utf8)
private let commitSpaceLt: [UInt8] = [UInt8](" <".utf8)
private let commitGtSpace: [UInt8] = [UInt8]("> ".utf8)

/// Serialize a commit to its raw data representation (without the object header)
public func serializeCommit(
    treeId: OID,
    parentIds: [OID],
    author: Signature,
    committer: Signature,
    message: String,
    messageEncoding: String? = nil
) -> Data {
    var buf = [UInt8]()
    buf.reserveCapacity(256 + message.count)

    buf.append(contentsOf: commitTreePrefix)
    treeId.appendHexBytes(to: &buf)
    buf.append(0x0A)

    for pid in parentIds {
        buf.append(contentsOf: commitParentPrefix)
        pid.appendHexBytes(to: &buf)
        buf.append(0x0A)
    }

    buf.append(contentsOf: commitAuthorPrefix)
    appendSignatureBytes(author, to: &buf)
    buf.append(0x0A)

    buf.append(contentsOf: commitCommitterPrefix)
    appendSignatureBytes(committer, to: &buf)
    buf.append(0x0A)

    if let enc = messageEncoding {
        buf.append(contentsOf: commitEncodingPrefix)
        var encStr = enc
        encStr.withUTF8 { buf.append(contentsOf: $0) }
        buf.append(0x0A)
    }

    buf.append(0x0A)
    var msg = message
    msg.withUTF8 { buf.append(contentsOf: $0) }
    return Data(buf)
}

/// Append a signature directly as bytes to a UInt8 buffer
private func appendSignatureBytes(_ sig: Signature, to buf: inout [UInt8]) {
    var name = sig.name
    name.withUTF8 { buf.append(contentsOf: $0) }
    buf.append(contentsOf: commitSpaceLt)
    var email = sig.email
    email.withUTF8 { buf.append(contentsOf: $0) }
    buf.append(contentsOf: commitGtSpace)
    appendInt64(sig.time, to: &buf)
    buf.append(0x20) // space
    buf.append(sig.offset >= 0 ? 0x2B : 0x2D) // '+' or '-'
    let absOffset = abs(sig.offset)
    let hours = absOffset / 60
    let minutes = absOffset % 60
    buf.append(UInt8(hours / 10) + 0x30)
    buf.append(UInt8(hours % 10) + 0x30)
    buf.append(UInt8(minutes / 10) + 0x30)
    buf.append(UInt8(minutes % 10) + 0x30)
}

/// Append decimal representation of an Int64 directly to a UInt8 buffer
private func appendInt64(_ value: Int64, to buf: inout [UInt8]) {
    if value == 0 {
        buf.append(0x30)
        return
    }
    var v = value
    if v < 0 {
        buf.append(0x2D) // '-'
        v = -v
    }
    let start = buf.count
    while v > 0 {
        buf.append(UInt8(v % 10) + 0x30)
        v /= 10
    }
    // Reverse digits in-place
    var lo = start
    var hi = buf.count - 1
    while lo < hi {
        let tmp = buf[lo]
        buf[lo] = buf[hi]
        buf[hi] = tmp
        lo += 1
        hi -= 1
    }
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
