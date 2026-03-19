/// MuonGit - Three-way merge with conflict detection
/// Parity: libgit2 src/libgit2/merge.c
import Foundation

/// A region in the merge result.
public enum MergeRegion: Equatable {
    case clean([String])
    case resolved([String])
    case conflict(base: [String], ours: [String], theirs: [String])
}

/// Result of a three-way merge.
public struct MergeResult {
    public let regions: [MergeRegion]
    public let hasConflicts: Bool

    /// Produce the merged text with conflict markers.
    public func toStringWithMarkers() -> String {
        var out = ""
        for region in regions {
            switch region {
            case .clean(let lines), .resolved(let lines):
                for line in lines { out += line + "\n" }
            case .conflict(_, let ours, let theirs):
                out += "<<<<<<< ours\n"
                for line in ours { out += line + "\n" }
                out += "=======\n"
                for line in theirs { out += line + "\n" }
                out += ">>>>>>> theirs\n"
            }
        }
        return out
    }

    /// Produce clean merged text. Returns nil if there are conflicts.
    public func toCleanString() -> String? {
        if hasConflicts { return nil }
        return toStringWithMarkers()
    }
}

/// Perform a three-way merge of text content.
public func merge3(base: String, ours: String, theirs: String) -> MergeResult {
    let baseLines = splitLines(base)
    let oursLines = splitLines(ours)
    let theirsLines = splitLines(theirs)

    let diffOurs = diff3Segments(base: baseLines, modified: oursLines)
    let diffTheirs = diff3Segments(base: baseLines, modified: theirsLines)

    let oursChanges = collectChanges(diffOurs)
    let theirsChanges = collectChanges(diffTheirs)

    var regions: [MergeRegion] = []
    var hasConflicts = false
    var basePos = 0
    var oi = 0, ti = 0

    while true {
        let nextOurs = oi < oursChanges.count ? oursChanges[oi].start : nil
        let nextTheirs = ti < theirsChanges.count ? theirsChanges[ti].start : nil

        guard let next: Int = {
            switch (nextOurs, nextTheirs) {
            case let (a?, b?): return min(a, b)
            case let (a?, nil): return a
            case let (nil, b?): return b
            case (nil, nil): return nil
            }
        }() else { break }

        if next > basePos {
            let clean = Array(baseLines[basePos..<next])
            if !clean.isEmpty { regions.append(.clean(clean)) }
            basePos = next
        }

        let oursHere = (oi < oursChanges.count && oursChanges[oi].start == basePos) ? oursChanges[oi] : nil
        let theirsHere = (ti < theirsChanges.count && theirsChanges[ti].start == basePos) ? theirsChanges[ti] : nil

        switch (oursHere, theirsHere) {
        case let (o?, t?):
            let maxEnd = max(o.start + o.count, t.start + t.count)
            if o.replacement == t.replacement {
                regions.append(.resolved(o.replacement))
            } else {
                hasConflicts = true
                let baseRegion = Array(baseLines[basePos..<min(maxEnd, baseLines.count)])
                regions.append(.conflict(base: baseRegion, ours: o.replacement, theirs: t.replacement))
            }
            basePos = maxEnd
            oi += 1; ti += 1
        case let (o?, nil):
            regions.append(.resolved(o.replacement))
            basePos = o.start + o.count
            oi += 1
        case let (nil, t?):
            regions.append(.resolved(t.replacement))
            basePos = t.start + t.count
            ti += 1
        case (nil, nil):
            break
        }
    }

    if basePos < baseLines.count {
        let clean = Array(baseLines[basePos...])
        if !clean.isEmpty { regions.append(.clean(clean)) }
    }

    return MergeResult(regions: regions, hasConflicts: hasConflicts)
}

// MARK: - Internal helpers

private func splitLines(_ text: String) -> [String] {
    if text.isEmpty { return [] }
    var lines = text.components(separatedBy: "\n")
    if lines.last == "" { lines.removeLast() }
    return lines
}

private struct Change {
    let start: Int
    let count: Int
    let replacement: [String]
}

private enum Segment {
    case equal
    case delete
    case insert(String)
}

private func diff3Segments(base: [String], modified: [String]) -> [Segment] {
    let lcs = lcsTable(base, modified)
    var i = base.count, j = modified.count
    var result: [Segment] = []

    while i > 0 && j > 0 {
        if base[i - 1] == modified[j - 1] {
            result.append(.equal)
            i -= 1; j -= 1
        } else if lcs[i - 1][j] >= lcs[i][j - 1] {
            result.append(.delete)
            i -= 1
        } else {
            result.append(.insert(modified[j - 1]))
            j -= 1
        }
    }
    while i > 0 { result.append(.delete); i -= 1 }
    while j > 0 { result.append(.insert(modified[j - 1])); j -= 1 }
    result.reverse()
    return result
}

private func lcsTable(_ a: [String], _ b: [String]) -> [[Int]] {
    let n = a.count, m = b.count
    var dp = Array(repeating: Array(repeating: 0, count: m + 1), count: n + 1)
    for i in 1...max(n, 1) {
        guard i <= n else { break }
        for j in 1...max(m, 1) {
            guard j <= m else { break }
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1
            } else {
                dp[i][j] = max(dp[i - 1][j], dp[i][j - 1])
            }
        }
    }
    return dp
}

private func collectChanges(_ segments: [Segment]) -> [Change] {
    var changes: [Change] = []
    var basePos = 0
    var i = 0

    while i < segments.count {
        if case .equal = segments[i] {
            basePos += 1; i += 1
            continue
        }

        let start = basePos
        var deleted = 0
        var inserted: [String] = []

        inner: while i < segments.count {
            switch segments[i] {
            case .delete:
                deleted += 1; basePos += 1; i += 1
            case .insert(let line):
                inserted.append(line); i += 1
            case .equal:
                break inner
            }
        }

        changes.append(Change(start: start, count: deleted, replacement: inserted))
    }

    return changes
}
