/// MuonGit - Git describe: find the most recent tag reachable from a commit
/// Parity: libgit2 src/libgit2/describe.c
import Foundation

/// Strategy for finding tags in describe
public enum DescribeStrategy: Sendable {
    case `default`  // annotated tags only
    case tags       // all tags
    case all        // all refs
}

/// Options for describe
public struct DescribeOptions: Sendable {
    public var strategy: DescribeStrategy
    public var maxCandidates: Int
    public var pattern: String?
    public var onlyFollowFirstParent: Bool
    public var showCommitOidAsFallback: Bool

    public init(
        strategy: DescribeStrategy = .default,
        maxCandidates: Int = 10,
        pattern: String? = nil,
        onlyFollowFirstParent: Bool = false,
        showCommitOidAsFallback: Bool = false
    ) {
        self.strategy = strategy
        self.maxCandidates = maxCandidates
        self.pattern = pattern
        self.onlyFollowFirstParent = onlyFollowFirstParent
        self.showCommitOidAsFallback = showCommitOidAsFallback
    }
}

/// Options for formatting a describe result
public struct DescribeFormatOptions: Sendable {
    public var abbreviatedSize: Int
    public var alwaysUseLongFormat: Bool
    public var dirtySuffix: String?

    public init(abbreviatedSize: Int = 7, alwaysUseLongFormat: Bool = false, dirtySuffix: String? = nil) {
        self.abbreviatedSize = abbreviatedSize
        self.alwaysUseLongFormat = alwaysUseLongFormat
        self.dirtySuffix = dirtySuffix
    }
}

/// Result of a describe operation
public struct DescribeResult: Sendable {
    public let tagName: String?
    public let depth: Int
    public let commitId: OID
    public let exactMatch: Bool
    public let fallbackToId: Bool

    /// Format the describe result as a string
    public func format(options: DescribeFormatOptions = DescribeFormatOptions()) -> String {
        var result: String
        if fallbackToId {
            result = String(commitId.hex.prefix(options.abbreviatedSize))
        } else if let tagName = tagName {
            if exactMatch && !options.alwaysUseLongFormat {
                result = tagName
            } else {
                let abbrev = String(commitId.hex.prefix(options.abbreviatedSize))
                result = "\(tagName)-\(depth)-g\(abbrev)"
            }
        } else {
            result = String(commitId.hex.prefix(options.abbreviatedSize))
        }
        if let suffix = options.dirtySuffix {
            result += suffix
        }
        return result
    }
}

/// A tag/ref candidate for describe
private struct TagCandidate {
    let name: String
    let priority: Int  // 2=annotated, 1=lightweight, 0=other
    let commitOid: OID
}

/// Describe a commit — find the most recent tag reachable from it
public func describe(gitDir: String, commitOid: OID, options: DescribeOptions = DescribeOptions()) throws -> DescribeResult {
    let candidates = try collectCandidates(gitDir: gitDir, options: options)

    // Check if commit itself is tagged
    if let candidate = candidates[commitOid.hex] {
        return DescribeResult(tagName: candidate.name, depth: 0, commitId: commitOid, exactMatch: true, fallbackToId: false)
    }

    // BFS from commit through parents
    var visited = Set<String>()
    var queue: [(OID, Int)] = [(commitOid, 0)]
    visited.insert(commitOid.hex)

    var best: (TagCandidate, Int)? = nil

    while !queue.isEmpty {
        let (oid, depth) = queue.removeFirst()

        if let candidate = candidates[oid.hex] {
            let dominated: Bool
            if let (currentBest, currentDepth) = best {
                dominated = depth < currentDepth || (depth == currentDepth && candidate.priority > currentBest.priority)
            } else {
                dominated = true
            }
            if dominated {
                best = (candidate, depth)
            }
            if let b = best, depth > b.1 + options.maxCandidates {
                break
            }
            continue
        }

        // Read commit and enqueue parents
        if let (objType, data) = try? readLooseObject(gitDir: gitDir, oid: oid),
           objType == .commit,
           let commit = try? parseCommit(oid: oid, data: data) {
            let parents = options.onlyFollowFirstParent ? Array(commit.parentIds.prefix(1)) : commit.parentIds
            for parentOid in parents {
                if !visited.contains(parentOid.hex) {
                    visited.insert(parentOid.hex)
                    queue.append((parentOid, depth + 1))
                }
            }
        }
    }

    if let (candidate, depth) = best {
        return DescribeResult(tagName: candidate.name, depth: depth, commitId: commitOid, exactMatch: false, fallbackToId: false)
    }

    if options.showCommitOidAsFallback {
        return DescribeResult(tagName: nil, depth: 0, commitId: commitOid, exactMatch: false, fallbackToId: true)
    }

    throw MuonGitError.notFound("no tag found for describe")
}

// MARK: - Internal helpers

private func collectCandidates(gitDir: String, options: DescribeOptions) throws -> [String: TagCandidate] {
    let refs = try listReferences(gitDir: gitDir)
    var candidates: [String: TagCandidate] = [:]

    for (refname, value) in refs {
        guard let (name, priority) = categorizeRef(refname, options: options) else { continue }

        // Apply pattern filter
        if let pattern = options.pattern, !globMatch(pattern: pattern, text: name) {
            continue
        }

        guard value.count == 40, value.allSatisfy({ $0.isHexDigit }) else { continue }
        let oid = OID(hex: value)

        let (commitOid, actualPriority) = peelToCommit(gitDir: gitDir, oid: oid, defaultPriority: priority)
        candidates[commitOid.hex] = TagCandidate(name: name, priority: actualPriority, commitOid: commitOid)
    }

    return candidates
}

private func categorizeRef(_ refname: String, options: DescribeOptions) -> (String, Int)? {
    switch options.strategy {
    case .default:
        if refname.hasPrefix("refs/tags/") {
            return (String(refname.dropFirst(10)), 2)
        }
        return nil
    case .tags:
        if refname.hasPrefix("refs/tags/") {
            return (String(refname.dropFirst(10)), 1)
        }
        return nil
    case .all:
        if refname.hasPrefix("refs/tags/") {
            return (String(refname.dropFirst(10)), 2)
        } else if refname.hasPrefix("refs/heads/") {
            return ("heads/\(refname.dropFirst(11))", 0)
        } else if refname.hasPrefix("refs/remotes/") {
            return ("remotes/\(refname.dropFirst(13))", 0)
        }
        return (refname, 0)
    }
}

private func peelToCommit(gitDir: String, oid: OID, defaultPriority: Int) -> (OID, Int) {
    guard let (objType, data) = try? readLooseObject(gitDir: gitDir, oid: oid) else {
        return (oid, defaultPriority)
    }
    if objType == .tag, let tag = try? parseTag(oid: oid, data: data) {
        return (tag.targetId, 2)
    }
    return (oid, defaultPriority)
}

/// Simple glob matching (supports * and ?)
func globMatch(pattern: String, text: String) -> Bool {
    let p = Array(pattern)
    let t = Array(text)
    return globMatchInner(p, 0, t, 0)
}

private func globMatchInner(_ pattern: [Character], _ pi: Int, _ text: [Character], _ ti: Int) -> Bool {
    if pi == pattern.count && ti == text.count { return true }
    if pi == pattern.count { return false }

    if pattern[pi] == "*" {
        return globMatchInner(pattern, pi + 1, text, ti)
            || (ti < text.count && globMatchInner(pattern, pi, text, ti + 1))
    }
    if ti < text.count {
        if pattern[pi] == "?" || pattern[pi] == text[ti] {
            return globMatchInner(pattern, pi + 1, text, ti + 1)
        }
    }
    return false
}
