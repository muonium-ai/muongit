/// MuonGit - Gitignore pattern matching
/// Parity: libgit2 src/libgit2/ignore.c
import Foundation

/// A single gitignore pattern.
private struct IgnorePattern {
    let pattern: String
    let negated: Bool
    let dirOnly: Bool
    let baseDir: String
}

/// Compiled gitignore rules for a repository.
public struct Ignore {
    private var patterns: [IgnorePattern] = []

    public init() {}

    /// Load gitignore rules for a repository.
    public static func load(gitDir: String, workdir: String) -> Ignore {
        var ignore = Ignore()

        // .git/info/exclude
        let excludePath = (gitDir as NSString).appendingPathComponent("info/exclude")
        if let content = try? String(contentsOfFile: excludePath, encoding: .utf8) {
            ignore.addPatterns(content, baseDir: "")
        }

        // Root .gitignore
        let gitignorePath = (workdir as NSString).appendingPathComponent(".gitignore")
        if let content = try? String(contentsOfFile: gitignorePath, encoding: .utf8) {
            ignore.addPatterns(content, baseDir: "")
        }

        return ignore
    }

    /// Load gitignore rules for a subdirectory.
    public mutating func loadForPath(workdir: String, relDir: String) {
        let dirPath = relDir.isEmpty ? workdir : (workdir as NSString).appendingPathComponent(relDir)
        let gitignorePath = (dirPath as NSString).appendingPathComponent(".gitignore")
        if let content = try? String(contentsOfFile: gitignorePath, encoding: .utf8) {
            let base = relDir.isEmpty ? "" : "\(relDir)/"
            addPatterns(content, baseDir: base)
        }
    }

    /// Parse and add patterns from gitignore content.
    public mutating func addPatterns(_ content: String, baseDir: String) {
        for line in content.components(separatedBy: "\n") {
            var trimmed = line
            // Remove trailing whitespace
            while trimmed.last?.isWhitespace == true { trimmed.removeLast() }

            if trimmed.isEmpty || trimmed.hasPrefix("#") { continue }

            var pattern = trimmed
            var negated = false
            var dirOnly = false

            if pattern.hasPrefix("!") {
                negated = true
                pattern = String(pattern.dropFirst())
            }

            if pattern.hasSuffix("/") {
                dirOnly = true
                pattern.removeLast()
            }

            if pattern.hasPrefix("/") {
                pattern = String(pattern.dropFirst())
            }

            if pattern.isEmpty { continue }

            patterns.append(IgnorePattern(pattern: pattern, negated: negated, dirOnly: dirOnly, baseDir: baseDir))
        }
    }

    /// Check if a path is ignored.
    public func isIgnored(_ path: String, isDir: Bool) -> Bool {
        var ignored = false
        for pat in patterns {
            if pat.dirOnly && !isDir { continue }
            if matches(pat, path: path) {
                ignored = !pat.negated
            }
        }
        return ignored
    }

    private func matches(_ pat: IgnorePattern, path: String) -> Bool {
        let pattern = pat.pattern

        if pattern.contains("/") {
            let fullPattern = "\(pat.baseDir)\(pattern)"
            return globMatch(fullPattern, path)
        }

        if !pat.baseDir.isEmpty {
            if path.hasPrefix(pat.baseDir) {
                let rel = String(path.dropFirst(pat.baseDir.count))
                return globMatch(pattern, rel) || matchBasename(pattern, rel)
            }
            return false
        }

        return matchBasename(pattern, path)
    }
}

private func matchBasename(_ pattern: String, _ path: String) -> Bool {
    let basename = path.split(separator: "/").last.map(String.init) ?? path
    return globMatch(pattern, basename)
}

/// Simple glob matcher supporting *, ?, [...], and **.
func globMatch(_ pattern: String, _ text: String) -> Bool {
    let p = Array(pattern.utf8)
    let t = Array(text.utf8)
    return globMatchInner(p, t)
}

private func globMatchInner(_ pattern: [UInt8], _ text: [UInt8]) -> Bool {
    var pi = 0, ti = 0
    var starPi = -1, starTi = 0

    while ti < text.count {
        if pi < pattern.count && pattern[pi] == UInt8(ascii: "*") {
            if pi + 1 < pattern.count && pattern[pi + 1] == UInt8(ascii: "*") {
                // '**' matches everything including '/'
                var rest = Array(pattern[(pi + 2)...])
                if rest.first == UInt8(ascii: "/") { rest = Array(rest.dropFirst()) }
                if rest.isEmpty { return true }
                for i in ti...text.count {
                    if globMatchInner(rest, Array(text[i...])) { return true }
                }
                return false
            }
            starPi = pi; starTi = ti; pi += 1
        } else if pi < pattern.count && pattern[pi] == UInt8(ascii: "?") && text[ti] != UInt8(ascii: "/") {
            pi += 1; ti += 1
        } else if pi < pattern.count && pattern[pi] == UInt8(ascii: "[") {
            if let (matched, consumed) = matchCharClass(Array(pattern[pi...]), text[ti]) {
                if matched { pi += consumed; ti += 1 }
                else if starPi >= 0 { starTi += 1; ti = starTi; pi = starPi + 1 }
                else { return false }
            } else if starPi >= 0 { starTi += 1; ti = starTi; pi = starPi + 1 }
            else { return false }
        } else if pi < pattern.count && pattern[pi] == text[ti] {
            pi += 1; ti += 1
        } else if starPi >= 0 && text[ti] != UInt8(ascii: "/") {
            starTi += 1; ti = starTi; pi = starPi + 1
        } else {
            return false
        }
    }

    while pi < pattern.count && pattern[pi] == UInt8(ascii: "*") { pi += 1 }
    return pi == pattern.count
}

private func matchCharClass(_ pattern: [UInt8], _ ch: UInt8) -> (Bool, Int)? {
    guard !pattern.isEmpty && pattern[0] == UInt8(ascii: "[") else { return nil }

    var i = 1
    var negate = false
    if i < pattern.count && (pattern[i] == UInt8(ascii: "!") || pattern[i] == UInt8(ascii: "^")) {
        negate = true; i += 1
    }

    var matched = false
    while i < pattern.count && pattern[i] != UInt8(ascii: "]") {
        if i + 2 < pattern.count && pattern[i + 1] == UInt8(ascii: "-") {
            if ch >= pattern[i] && ch <= pattern[i + 2] { matched = true }
            i += 3
        } else {
            if ch == pattern[i] { matched = true }
            i += 1
        }
    }

    guard i < pattern.count && pattern[i] == UInt8(ascii: "]") else { return nil }
    return (negate ? !matched : matched, i + 1)
}
