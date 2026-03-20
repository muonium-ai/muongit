/// Commit graph walking for log-style traversal.
/// Parity target: libgit2 `git_revwalk`
import Foundation

public struct RevwalkSort: OptionSet, Sendable {
    public let rawValue: Int

    public init(rawValue: Int) {
        self.rawValue = rawValue
    }

    public static let topological = RevwalkSort(rawValue: 1 << 0)
    public static let time = RevwalkSort(rawValue: 1 << 1)
    public static let reverse = RevwalkSort(rawValue: 1 << 2)
}

public final class Revwalk {
    public let gitDir: String

    private var roots: [OID] = []
    private var hidden: [OID] = []
    private var sortMode: RevwalkSort = []
    private var firstParentOnly = false
    private var prepared: [OID]?
    private var cursor = 0

    public init(gitDir: String) {
        self.gitDir = gitDir
    }

    public func reset() {
        roots.removeAll()
        hidden.removeAll()
        firstParentOnly = false
        invalidate()
    }

    public func sorting(_ sortMode: RevwalkSort) {
        self.sortMode = sortMode
        invalidate()
    }

    public func simplifyFirstParent() {
        firstParentOnly = true
        invalidate()
    }

    public func push(_ oid: OID) {
        roots.append(oid)
        invalidate()
    }

    public func pushHead() throws {
        push(try resolveReference(gitDir: gitDir, name: "HEAD"))
    }

    public func pushRef(_ refName: String) throws {
        push(try resolveReference(gitDir: gitDir, name: refName))
    }

    public func hide(_ oid: OID) {
        hidden.append(oid)
        invalidate()
    }

    public func hideHead() throws {
        hide(try resolveReference(gitDir: gitDir, name: "HEAD"))
    }

    public func hideRef(_ refName: String) throws {
        hide(try resolveReference(gitDir: gitDir, name: refName))
    }

    public func push(_ revSpec: RevSpec) throws {
        if !revSpec.isRange {
            guard let oid = revSpec.to else {
                throw MuonGitError.invalidSpec("revspec is missing a target commit")
            }
            push(oid)
            return
        }

        guard let from = revSpec.from else {
            throw MuonGitError.invalidSpec("range is missing a left-hand side")
        }
        guard let to = revSpec.to else {
            throw MuonGitError.invalidSpec("range is missing a right-hand side")
        }

        if revSpec.usesMergeBase {
            push(from)
            push(to)
            for base in try revwalkMergeBases(gitDir: gitDir, left: from, right: to) {
                hide(base)
            }
        } else {
            push(to)
            hide(from)
        }
    }

    public func pushRange(_ spec: String) throws {
        let revSpec = try revparse(gitDir: gitDir, spec: spec)
        guard revSpec.isRange else {
            throw MuonGitError.invalidSpec("'\(spec)' is not a revision range")
        }
        try push(revSpec)
    }

    public func next() throws -> OID? {
        try prepare()
        guard let prepared, cursor < prepared.count else {
            return nil
        }
        let oid = prepared[cursor]
        cursor += 1
        return oid
    }

    public func allOids() throws -> [OID] {
        try prepare()
        return prepared ?? []
    }

    private func invalidate() {
        prepared = nil
        cursor = 0
    }

    private func prepare() throws {
        guard prepared == nil else { return }

        let hiddenSet = try collectRevisionAncestors(
            gitDir: gitDir,
            starts: hidden,
            firstParentOnly: firstParentOnly
        )
        let commits = try collectVisibleRevisionCommits(
            gitDir: gitDir,
            roots: roots,
            hidden: hiddenSet,
            firstParentOnly: firstParentOnly
        )

        var ordered: [OID]
        if sortMode.contains(.topological) {
            ordered = topoSortRevisionCommits(
                commits: commits,
                sortMode: sortMode,
                firstParentOnly: firstParentOnly
            )
        } else {
            ordered = Array(commits.keys).sorted {
                compareRevisionCommits($0, $1, commits: commits, sortMode: sortMode) == .orderedAscending
            }
        }

        if sortMode.contains(.reverse) {
            ordered.reverse()
        }

        prepared = ordered
        cursor = 0
    }
}

private func collectRevisionAncestors(
    gitDir: String,
    starts: [OID],
    firstParentOnly: Bool
) throws -> Set<OID> {
    var visited = Set<OID>()
    var queue = starts

    for oid in starts {
        visited.insert(oid)
    }

    while !queue.isEmpty {
        let oid = queue.removeFirst()
        let commit = try revisionReadCommit(gitDir: gitDir, oid: oid)
        for parent in revisionSelectedParents(commit, firstParentOnly: firstParentOnly) where visited.insert(parent).inserted {
            queue.append(parent)
        }
    }

    return visited
}

private func collectVisibleRevisionCommits(
    gitDir: String,
    roots: [OID],
    hidden: Set<OID>,
    firstParentOnly: Bool
) throws -> [OID: Commit] {
    var commits: [OID: Commit] = [:]
    var queue: [OID] = []
    var seen = Set<OID>()

    for oid in roots where !hidden.contains(oid) {
        if seen.insert(oid).inserted {
            queue.append(oid)
        }
    }

    while !queue.isEmpty {
        let oid = queue.removeFirst()
        if hidden.contains(oid) {
            continue
        }

        let commit = try revisionReadCommit(gitDir: gitDir, oid: oid)
        for parent in revisionSelectedParents(commit, firstParentOnly: firstParentOnly)
        where !hidden.contains(parent) && seen.insert(parent).inserted {
            queue.append(parent)
        }
        commits[oid] = commit
    }

    return commits
}

private func topoSortRevisionCommits(
    commits: [OID: Commit],
    sortMode: RevwalkSort,
    firstParentOnly: Bool
) -> [OID] {
    var childCounts = Dictionary(uniqueKeysWithValues: commits.keys.map { ($0, 0) })

    for commit in commits.values {
        for parent in revisionSelectedParents(commit, firstParentOnly: firstParentOnly) where childCounts[parent] != nil {
            childCounts[parent, default: 0] += 1
        }
    }

    var ready = childCounts
        .filter { $0.value == 0 }
        .map(\.key)
        .sorted {
            compareRevisionCommits($0, $1, commits: commits, sortMode: sortMode) == .orderedAscending
        }

    var ordered: [OID] = []
    ordered.reserveCapacity(commits.count)

    while !ready.isEmpty {
        let oid = ready.removeFirst()
        ordered.append(oid)

        guard let commit = commits[oid] else { continue }
        for parent in revisionSelectedParents(commit, firstParentOnly: firstParentOnly) where childCounts[parent] != nil {
            childCounts[parent, default: 0] -= 1
            if childCounts[parent] == 0 {
                ready.append(parent)
            }
        }
        ready.sort {
            compareRevisionCommits($0, $1, commits: commits, sortMode: sortMode) == .orderedAscending
        }
    }

    return ordered
}

private func compareRevisionCommits(
    _ left: OID,
    _ right: OID,
    commits: [OID: Commit],
    sortMode: RevwalkSort
) -> ComparisonResult {
    let usesTime = sortMode.isEmpty || sortMode.contains(.time)
    if usesTime {
        let leftTime = commits[left]?.committer.time ?? 0
        let rightTime = commits[right]?.committer.time ?? 0
        if leftTime != rightTime {
            return leftTime > rightTime ? .orderedAscending : .orderedDescending
        }
    }
    if left.hex == right.hex {
        return .orderedSame
    }
    return left.hex < right.hex ? .orderedAscending : .orderedDescending
}

private func revisionSelectedParents(_ commit: Commit, firstParentOnly: Bool) -> [OID] {
    if firstParentOnly, let first = commit.parentIds.first {
        return [first]
    }
    return commit.parentIds
}

private func revwalkMergeBases(gitDir: String, left: OID, right: OID) throws -> [OID] {
    if left == right {
        return [left]
    }

    let leftAncestors = try collectRevisionAncestors(gitDir: gitDir, starts: [left], firstParentOnly: false)
    var common: [OID] = []
    var visited: Set<OID> = [right]
    var queue: [OID] = [right]

    while !queue.isEmpty {
        let oid = queue.removeFirst()
        if leftAncestors.contains(oid) {
            common.append(oid)
            continue
        }

        let commit = try revisionReadCommit(gitDir: gitDir, oid: oid)
        for parent in commit.parentIds where visited.insert(parent).inserted {
            queue.append(parent)
        }
    }

    var best = common
    for candidate in common {
        let candidateAncestors = try collectRevisionAncestors(
            gitDir: gitDir,
            starts: [candidate],
            firstParentOnly: false
        )
        best = best.filter { $0 == candidate || !candidateAncestors.contains($0) }
    }

    return Array(Set(best)).sorted { $0.hex < $1.hex }
}
