// Attributes.swift - Gitattributes support
// Parity: libgit2 src/libgit2/attr_file.c

import Foundation

/// A single attribute value.
public enum AttrValue: Equatable {
    case set
    case unset
    case value(String)
}

/// A single attribute rule.
struct AttrRule {
    let pattern: String
    let attrs: [(String, AttrValue)]
}

/// Compiled gitattributes rules for a repository.
public class Attributes {
    private var rules: [AttrRule] = []

    public init() {}

    /// Load from a file path.
    public static func load(path: String) -> Attributes {
        let attrs = Attributes()
        if let content = try? String(contentsOfFile: path, encoding: .utf8) {
            attrs.parse(content)
        }
        return attrs
    }

    /// Load for a repository (worktree .gitattributes + info/attributes).
    public static func loadForRepo(gitDir: String, workdir: String?) -> Attributes {
        let attrs = Attributes()

        if let wd = workdir {
            let worktreeAttrs = (wd as NSString).appendingPathComponent(".gitattributes")
            if let content = try? String(contentsOfFile: worktreeAttrs, encoding: .utf8) {
                attrs.parse(content)
            }
        }

        let infoAttrs = (gitDir as NSString).appendingPathComponent("info/attributes")
        if let content = try? String(contentsOfFile: infoAttrs, encoding: .utf8) {
            attrs.parse(content)
        }

        return attrs
    }

    /// Parse gitattributes content.
    public func parse(_ content: String) {
        for line in content.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty || trimmed.hasPrefix("#") { continue }
            if let rule = parseAttrLine(trimmed) {
                rules.append(rule)
            }
        }
    }

    /// Get the value of a specific attribute for a path.
    public func get(_ path: String, attr: String) -> AttrValue? {
        var result: AttrValue? = nil
        for rule in rules {
            if attrPathMatch(path, pattern: rule.pattern) {
                for (name, value) in rule.attrs {
                    if name == attr { result = value }
                }
            }
        }
        return result
    }

    /// Get all attributes for a path.
    public func getAll(_ path: String) -> [(String, AttrValue)] {
        var map: [String: AttrValue] = [:]
        for rule in rules {
            if attrPathMatch(path, pattern: rule.pattern) {
                for (name, value) in rule.attrs {
                    map[name] = value
                }
            }
        }
        return map.sorted { $0.key < $1.key }.map { ($0.key, $0.value) }
    }

    /// Check if a path is binary.
    public func isBinary(_ path: String) -> Bool {
        if get(path, attr: "binary") == .set { return true }
        if get(path, attr: "diff") == .unset { return true }
        if get(path, attr: "text") == .unset { return true }
        return false
    }

    /// Get eol setting for a path.
    public func eol(_ path: String) -> String? {
        if case .value(let v) = get(path, attr: "eol") { return v }
        return nil
    }

    var ruleCount: Int { rules.count }
}

private func parseAttrLine(_ line: String) -> AttrRule? {
    guard let spaceIdx = line.firstIndex(where: { $0 == " " || $0 == "\t" }) else { return nil }
    let pattern = String(line[line.startIndex..<spaceIdx])
    let attrStr = String(line[line.index(after: spaceIdx)...])

    if pattern.isEmpty { return nil }
    let attrs = parseAttrs(attrStr)
    if attrs.isEmpty { return nil }
    return AttrRule(pattern: pattern, attrs: attrs)
}

private func parseAttrs(_ s: String) -> [(String, AttrValue)] {
    var attrs: [(String, AttrValue)] = []
    for token in s.split(separator: " ").map(String.init) where !token.isEmpty {
        if token == "binary" {
            attrs.append(("binary", .set))
            attrs.append(("diff", .unset))
            attrs.append(("merge", .unset))
            attrs.append(("text", .unset))
            continue
        }
        if token.hasPrefix("-") {
            attrs.append((String(token.dropFirst()), .unset))
        } else if let eqIdx = token.firstIndex(of: "=") {
            let name = String(token[token.startIndex..<eqIdx])
            let value = String(token[token.index(after: eqIdx)...])
            attrs.append((name, .value(value)))
        } else {
            attrs.append((token, .set))
        }
    }
    return attrs
}

private func attrPathMatch(_ path: String, pattern: String) -> Bool {
    if pattern.contains("/") {
        return attrGlobMatch(pattern, path)
    } else {
        let basename = path.split(separator: "/").last.map(String.init) ?? path
        return attrGlobMatch(pattern, basename)
    }
}

private func attrGlobMatch(_ pattern: String, _ text: String) -> Bool {
    let pat = Array(pattern)
    let txt = Array(text)
    var pi = 0, ti = 0
    var starPi = -1, starTi = 0

    while ti < txt.count {
        if pi < pat.count && pat[pi] == "?" {
            pi += 1; ti += 1
        } else if pi < pat.count && pat[pi] == "*" {
            starPi = pi; starTi = ti; pi += 1
        } else if pi < pat.count && pat[pi] == "[" {
            if let (matched, consumed) = attrMatchCharClass(Array(pat[pi...]), txt[ti]) {
                if matched { pi += consumed; ti += 1 }
                else if starPi >= 0 { pi = starPi + 1; starTi += 1; ti = starTi }
                else { return false }
            } else if starPi >= 0 { pi = starPi + 1; starTi += 1; ti = starTi }
            else { return false }
        } else if pi < pat.count && pat[pi] == txt[ti] {
            pi += 1; ti += 1
        } else if starPi >= 0 {
            pi = starPi + 1; starTi += 1; ti = starTi
        } else { return false }
    }
    while pi < pat.count && pat[pi] == "*" { pi += 1 }
    return pi == pat.count
}

private func attrMatchCharClass(_ pat: [Character], _ ch: Character) -> (Bool, Int)? {
    guard !pat.isEmpty, pat[0] == "[" else { return nil }
    var i = 1
    let negated = i < pat.count && pat[i] == "!"
    if negated { i += 1 }
    var matched = false
    while i < pat.count && pat[i] != "]" {
        if i + 2 < pat.count && pat[i + 1] == "-" {
            if ch >= pat[i] && ch <= pat[i + 2] { matched = true }
            i += 3
        } else {
            if pat[i] == ch { matched = true }
            i += 1
        }
    }
    guard i < pat.count, pat[i] == "]" else { return nil }
    if negated { matched = !matched }
    return (matched, i + 1)
}
