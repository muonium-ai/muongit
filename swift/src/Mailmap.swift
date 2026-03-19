/// MuonGit - Mailmap: email/name mapping for canonical author identities
/// Parity: libgit2 src/libgit2/mailmap.c
import Foundation

/// A single mailmap entry
public struct MailmapEntry: Sendable {
    public let realName: String?
    public let realEmail: String?
    public let replaceName: String?
    public let replaceEmail: String
}

/// A mailmap holding name/email mappings
public struct Mailmap: Sendable {
    private var entries: [MailmapEntry] = []

    public init() {}

    /// Load mailmap from a file
    public static func load(path: String) throws -> Mailmap {
        var mm = Mailmap()
        let fm = FileManager.default
        if fm.fileExists(atPath: path) {
            let content = try String(contentsOfFile: path, encoding: .utf8)
            mm.parse(content)
        }
        return mm
    }

    /// Load mailmap for a repository workdir
    public static func loadForRepo(workdir: String) throws -> Mailmap {
        let path = (workdir as NSString).appendingPathComponent(".mailmap")
        return try load(path: path)
    }

    /// Parse mailmap content
    public mutating func parse(_ content: String) {
        for line in content.components(separatedBy: "\n") {
            let line = line.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            if let entry = parseMailmapLine(line) {
                entries.append(entry)
            }
        }
        entries.sort { a, b in
            let emailCmp = a.replaceEmail.lowercased().compare(b.replaceEmail.lowercased())
            if emailCmp != .orderedSame { return emailCmp == .orderedAscending }
            let aName = (a.replaceName ?? "").lowercased()
            let bName = (b.replaceName ?? "").lowercased()
            return aName < bName
        }
    }

    /// Add a mapping entry
    public mutating func addEntry(_ entry: MailmapEntry) {
        entries.append(entry)
    }

    /// Resolve a name/email pair to canonical values
    public func resolve(name: String, email: String) -> (name: String, email: String) {
        let emailLower = email.lowercased()

        // First try exact match with both name and email
        for entry in entries {
            if entry.replaceEmail.lowercased() == emailLower {
                if let rn = entry.replaceName, rn.lowercased() == name.lowercased() {
                    let resolvedName = entry.realName ?? name
                    let resolvedEmail = entry.realEmail ?? email
                    return (resolvedName, resolvedEmail)
                }
            }
        }

        // Then try email-only match
        for entry in entries {
            if entry.replaceEmail.lowercased() == emailLower && entry.replaceName == nil {
                let resolvedName = entry.realName ?? name
                let resolvedEmail = entry.realEmail ?? email
                return (resolvedName, resolvedEmail)
            }
        }

        return (name, email)
    }

    /// Resolve a signature to canonical values
    public func resolveSignature(_ sig: Signature) -> Signature {
        let (name, email) = resolve(name: sig.name, email: sig.email)
        return Signature(name: name, email: email, time: sig.time, offset: sig.offset)
    }

    /// Number of entries
    public var count: Int { entries.count }

    /// Whether the mailmap is empty
    public var isEmpty: Bool { entries.isEmpty }
}

// MARK: - Parsing

private func parseMailmapLine(_ line: String) -> MailmapEntry? {
    var emails: [String] = []
    var names: [String] = []
    var currentName = ""
    var inEmail = false
    var currentEmail = ""

    for ch in line {
        switch ch {
        case "<":
            inEmail = true
            currentEmail = ""
            let name = currentName.trimmingCharacters(in: .whitespaces)
            if !name.isEmpty {
                names.append(name)
            }
            currentName = ""
        case ">":
            inEmail = false
            emails.append(currentEmail.trimmingCharacters(in: .whitespaces))
        default:
            if inEmail {
                currentEmail.append(ch)
            } else {
                currentName.append(ch)
            }
        }
    }

    guard emails.count == 2 else { return nil }

    let realName = names.isEmpty || names[0].isEmpty ? nil : names[0]
    let replaceName = names.count > 1 && !names[1].isEmpty ? names[1] : nil
    let realEmail = emails[0].isEmpty ? nil : emails[0]

    return MailmapEntry(
        realName: realName,
        realEmail: realEmail,
        replaceName: replaceName,
        replaceEmail: emails[1]
    )
}
