/// Commit-oriented revision parsing for common Git revision expressions.
/// Parity target: libgit2 `git_revparse_single` / `git_revparse`
import Foundation

public struct RevSpec: Sendable, Equatable {
    public let from: OID?
    public let to: OID?
    public let isRange: Bool
    public let usesMergeBase: Bool

    public init(from: OID?, to: OID?, isRange: Bool, usesMergeBase: Bool) {
        self.from = from
        self.to = to
        self.isRange = isRange
        self.usesMergeBase = usesMergeBase
    }
}

/// Resolve a common revision expression to a commit OID.
///
/// Supported subset:
/// - full OIDs
/// - refs and short refs like `main`, `tags/v1`, `origin/main`
/// - `HEAD^`, `HEAD^N`, `HEAD~N`
public func resolveRevision(gitDir: String, spec: String) throws -> OID {
    let trimmed = spec.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        throw MuonGitError.invalidSpec("empty revision spec")
    }
    if trimmed.contains("...") || isTwoDotRange(trimmed) {
        throw MuonGitError.invalidSpec("range '\(trimmed)' does not resolve to a single revision")
    }

    let (baseSpec, suffix) = try splitBaseAndSuffix(trimmed)
    var current = try readObject(gitDir: gitDir, oid: resolveRevisionBase(gitDir: gitDir, spec: baseSpec))

    let chars = Array(suffix)
    var index = 0
    while index < chars.count {
        switch chars[index] {
        case "~":
            index += 1
            let start = index
            while index < chars.count, chars[index].wholeNumberValue != nil {
                index += 1
            }
            let count = start == index ? 1 : Int(String(chars[start..<index])) ?? -1
            guard count >= 0 else {
                throw MuonGitError.invalidSpec("invalid ancestry operator in '\(trimmed)'")
            }
            for _ in 0..<count {
                let commit = try peelRevisionCommit(gitDir: gitDir, object: current, spec: trimmed)
                guard let parent = commit.parentIds.first else {
                    throw MuonGitError.invalidSpec("revision '\(trimmed)' has no first parent")
                }
                current = try readObject(gitDir: gitDir, oid: parent)
            }
        case "^":
            index += 1
            let start = index
            while index < chars.count, chars[index].wholeNumberValue != nil {
                index += 1
            }
            let parentIndex = start == index ? 1 : Int(String(chars[start..<index])) ?? -1
            guard parentIndex >= 0 else {
                throw MuonGitError.invalidSpec("invalid parent selector in '\(trimmed)'")
            }
            let commit = try peelRevisionCommit(gitDir: gitDir, object: current, spec: trimmed)
            if parentIndex == 0 {
                current = try readObject(gitDir: gitDir, oid: commit.oid)
                continue
            }
            guard commit.parentIds.count >= parentIndex else {
                throw MuonGitError.invalidSpec("revision '\(trimmed)' has no parent \(parentIndex)")
            }
            current = try readObject(gitDir: gitDir, oid: commit.parentIds[parentIndex - 1])
        default:
            throw MuonGitError.invalidSpec("unsupported revision syntax '\(trimmed)'")
        }
    }

    return try peelRevisionCommit(gitDir: gitDir, object: current, spec: trimmed).oid
}

public func revparseSingle(gitDir: String, spec: String) throws -> GitObject {
    try readObject(gitDir: gitDir, oid: resolveRevision(gitDir: gitDir, spec: spec))
}

public func revparse(gitDir: String, spec: String) throws -> RevSpec {
    let trimmed = spec.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        throw MuonGitError.invalidSpec("empty revision spec")
    }

    if let (left, right) = splitRange(trimmed, operator: "...") {
        return RevSpec(
            from: try resolveRevision(gitDir: gitDir, spec: left),
            to: try resolveRevision(gitDir: gitDir, spec: right),
            isRange: true,
            usesMergeBase: true
        )
    }

    if let (left, right) = splitTwoDotRange(trimmed) {
        return RevSpec(
            from: try resolveRevision(gitDir: gitDir, spec: left),
            to: try resolveRevision(gitDir: gitDir, spec: right),
            isRange: true,
            usesMergeBase: false
        )
    }

    return RevSpec(
        from: nil,
        to: try resolveRevision(gitDir: gitDir, spec: trimmed),
        isRange: false,
        usesMergeBase: false
    )
}

func revisionReadCommit(gitDir: String, oid: OID) throws -> Commit {
    let object = try readObject(gitDir: gitDir, oid: oid)
    guard object.objectType == .commit else {
        throw MuonGitError.invalidSpec("revision '\(oid.hex)' is not a commit")
    }
    return try object.asCommit()
}

private func resolveRevisionBase(gitDir: String, spec: String) throws -> OID {
    let trimmed = spec.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        throw MuonGitError.invalidSpec("missing base revision")
    }

    if looksLikeFullOID(trimmed) {
        let oid = OID(hex: trimmed)
        if (try? readObject(gitDir: gitDir, oid: oid)) != nil {
            return oid
        }
    }

    for candidate in revisionReferenceCandidates(trimmed) {
        if let oid = try? resolveReference(gitDir: gitDir, name: candidate) {
            return oid
        }
    }

    throw MuonGitError.notFound("could not resolve revision '\(trimmed)'")
}

private func peelRevisionCommit(gitDir: String, object: GitObject, spec: String) throws -> Commit {
    let peeled = object.objectType == .tag ? try object.peel(gitDir: gitDir) : object
    guard peeled.objectType == .commit else {
        throw MuonGitError.invalidSpec("revision '\(spec)' does not resolve to a commit")
    }
    return try peeled.asCommit()
}

private func splitBaseAndSuffix(_ spec: String) throws -> (String, Substring) {
    let index = spec.firstIndex { $0 == "^" || $0 == "~" } ?? spec.endIndex
    let base = String(spec[..<index])
    guard !base.isEmpty else {
        throw MuonGitError.invalidSpec("missing base revision in '\(spec)'")
    }
    return (base, spec[index...])
}

private func splitRange(_ spec: String, operator: String) -> (String, String)? {
    guard let range = spec.range(of: `operator`) else {
        return nil
    }
    let left = String(spec[..<range.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
    let right = String(spec[range.upperBound...]).trimmingCharacters(in: .whitespacesAndNewlines)
    guard !left.isEmpty, !right.isEmpty else {
        return nil
    }
    return (left, right)
}

private func splitTwoDotRange(_ spec: String) -> (String, String)? {
    guard !spec.contains("...") else { return nil }
    return splitRange(spec, operator: "..")
}

private func isTwoDotRange(_ spec: String) -> Bool {
    splitTwoDotRange(spec) != nil
}

private func looksLikeFullOID(_ spec: String) -> Bool {
    spec.count == OID.sha1HexLength
        && spec.unicodeScalars.allSatisfy { scalar in
            let value = scalar.value
            return (48...57).contains(value) || (65...70).contains(value) || (97...102).contains(value)
        }
}

private func revisionReferenceCandidates(_ spec: String) -> [String] {
    var candidates = [spec]
    if !spec.hasPrefix("refs/") {
        candidates.append("refs/\(spec)")
        candidates.append("refs/heads/\(spec)")
        candidates.append("refs/tags/\(spec)")
        candidates.append("refs/remotes/\(spec)")
    }
    return candidates
}
