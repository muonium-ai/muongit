/// MuonGit - Pathspec pattern matching
/// Parity: libgit2 src/libgit2/pathspec.c
import Foundation

/// Flags for pathspec matching behavior
public struct PathspecFlags: Sendable {
    public var ignoreCase: Bool
    public var noGlob: Bool
    public var noMatchError: Bool
    public var findFailures: Bool

    public init(ignoreCase: Bool = false, noGlob: Bool = false, noMatchError: Bool = false, findFailures: Bool = false) {
        self.ignoreCase = ignoreCase
        self.noGlob = noGlob
        self.noMatchError = noMatchError
        self.findFailures = findFailures
    }
}

/// A single pathspec pattern
private struct PathspecPattern {
    let pattern: String
    let negated: Bool
    let hasWildcard: Bool
    let matchAll: Bool
}

/// Compiled pathspec for matching file paths
public struct Pathspec: Sendable {
    private let patterns: [PathspecPattern]

    /// Create a new pathspec from a list of patterns
    public init(patterns: [String]) {
        self.patterns = patterns.map { Self.parsePattern($0) }
    }

    /// Check if a path matches this pathspec
    public func matchesPath(_ path: String, flags: PathspecFlags = PathspecFlags()) -> Bool {
        if patterns.isEmpty { return true }

        var matched = false
        for pattern in patterns {
            if pattern.matchAll {
                matched = !pattern.negated
                continue
            }

            let doesMatch: Bool
            if flags.noGlob {
                doesMatch = pathMatchesLiteral(path: path, pattern: pattern.pattern, ignoreCase: flags.ignoreCase)
            } else {
                doesMatch = pathMatchesGlob(path: path, pattern: pattern.pattern, ignoreCase: flags.ignoreCase)
            }

            if doesMatch {
                matched = !pattern.negated
            }
        }
        return matched
    }

    /// Match this pathspec against a list of paths
    public func matchPaths(_ paths: [String], flags: PathspecFlags = PathspecFlags()) -> PathspecMatchResult {
        var matches: [String] = []
        var matchedPatterns = [Bool](repeating: false, count: patterns.count)

        for path in paths {
            if matchesPath(path, flags: flags) {
                matches.append(path)
            }
            for (i, pattern) in patterns.enumerated() {
                if !pattern.negated && patternMatchesPath(path: path, pattern: pattern, flags: flags) {
                    matchedPatterns[i] = true
                }
            }
        }

        var failures: [String] = []
        if flags.findFailures {
            for (i, pattern) in patterns.enumerated() {
                if !pattern.negated && !matchedPatterns[i] {
                    failures.append(pattern.pattern)
                }
            }
        }

        return PathspecMatchResult(matches: matches, failures: failures)
    }

    public var count: Int { patterns.count }
    public var isEmpty: Bool { patterns.isEmpty }

    // MARK: - Parsing

    private static func parsePattern(_ pat: String) -> PathspecPattern {
        var pattern = pat
        var negated = false

        if pattern.hasPrefix("!") {
            negated = true
            pattern = String(pattern.dropFirst())
        } else if pattern.hasPrefix("\\!") {
            pattern = String(pattern.dropFirst())
        }

        if pattern.hasPrefix("/") {
            pattern = String(pattern.dropFirst())
        }

        let matchAll = pattern == "*" || pattern.isEmpty
        let hasWildcard = pattern.contains("*") || pattern.contains("?") || pattern.contains("[")

        return PathspecPattern(pattern: pattern, negated: negated, hasWildcard: hasWildcard, matchAll: matchAll)
    }
}

/// Result of matching a pathspec against a list of paths
public struct PathspecMatchResult: Sendable {
    public let matches: [String]
    public let failures: [String]
}

// MARK: - Internal matching

private func patternMatchesPath(path: String, pattern: PathspecPattern, flags: PathspecFlags) -> Bool {
    if pattern.matchAll { return true }
    if flags.noGlob {
        return pathMatchesLiteral(path: path, pattern: pattern.pattern, ignoreCase: flags.ignoreCase)
    }
    return pathMatchesGlob(path: path, pattern: pattern.pattern, ignoreCase: flags.ignoreCase)
}

private func pathMatchesLiteral(path: String, pattern: String, ignoreCase: Bool) -> Bool {
    let p = ignoreCase ? pattern.lowercased() : pattern
    let t = ignoreCase ? path.lowercased() : path

    if t == p { return true }
    if t.hasPrefix(p) && t.dropFirst(p.count).first == "/" { return true }
    return false
}

private func pathMatchesGlob(path: String, pattern: String, ignoreCase: Bool) -> Bool {
    let p = ignoreCase ? pattern.lowercased() : pattern
    let t = ignoreCase ? path.lowercased() : path

    // Handle ** (any number of path levels)
    if p.hasPrefix("**/") {
        let sub = String(p.dropFirst(3))
        if wildmatch(pattern: sub, text: t) { return true }
        var remaining = t
        while let idx = remaining.firstIndex(of: "/") {
            remaining = String(remaining[remaining.index(after: idx)...])
            if wildmatch(pattern: sub, text: remaining) { return true }
        }
        return false
    }

    // If pattern has no '/', match against basename
    if !p.contains("/") {
        let basename = t.split(separator: "/").last.map(String.init) ?? t
        if wildmatch(pattern: p, text: basename) { return true }
    }

    // Full path match
    if wildmatch(pattern: p, text: t) { return true }

    // Directory prefix
    let stripped = p.hasSuffix("/") ? String(p.dropLast()) : p
    if t.hasPrefix(stripped) && t.dropFirst(stripped.count).first == "/" { return true }

    return false
}

private func wildmatch(pattern: String, text: String) -> Bool {
    let p = Array(pattern)
    let t = Array(text)
    return wildmatchInner(p, 0, t, 0)
}

private func wildmatchInner(_ pattern: [Character], _ pi: Int, _ text: [Character], _ ti: Int) -> Bool {
    if pi == pattern.count && ti == text.count { return true }
    if pi == pattern.count { return false }

    if pattern[pi] == "*" {
        // * matches everything except /
        if wildmatchInner(pattern, pi + 1, text, ti) { return true }
        if ti < text.count && text[ti] != "/" {
            return wildmatchInner(pattern, pi, text, ti + 1)
        }
        return false
    }

    if ti >= text.count { return false }

    if pattern[pi] == "?" {
        return text[ti] != "/" && wildmatchInner(pattern, pi + 1, text, ti + 1)
    }

    if pattern[pi] == "[" {
        if let (matched, nextPi) = matchCharClass(pattern, pi + 1, text[ti]) {
            if matched { return wildmatchInner(pattern, nextPi, text, ti + 1) }
        }
        return false
    }

    if pattern[pi] == text[ti] {
        return wildmatchInner(pattern, pi + 1, text, ti + 1)
    }

    return false
}

private func matchCharClass(_ pattern: [Character], _ start: Int, _ ch: Character) -> (Bool, Int)? {
    var i = start
    var negated = false
    if i < pattern.count && pattern[i] == "!" {
        negated = true
        i += 1
    }

    var matched = false
    while i < pattern.count && pattern[i] != "]" {
        if i + 2 < pattern.count && pattern[i + 1] == "-" {
            if ch >= pattern[i] && ch <= pattern[i + 2] { matched = true }
            i += 3
        } else {
            if ch == pattern[i] { matched = true }
            i += 1
        }
    }

    guard i < pattern.count && pattern[i] == "]" else { return nil }
    let result = negated ? !matched : matched
    return (result, i + 1)
}
