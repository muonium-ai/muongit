/// MuonGit - Merge base computation
/// Parity: libgit2 src/libgit2/merge.c (git_merge_base)
import Foundation

/// Read and parse a commit from the object database.
private func readCommit(gitDir: String, oid: OID) throws -> Commit {
    try readObject(gitDir: gitDir, oid: oid).asCommit()
}

/// Collect all ancestors of a commit (including itself) via BFS.
private func ancestors(gitDir: String, oid: OID) throws -> Set<OID> {
    var visited = Set<OID>()
    var queue: [OID] = [oid]
    visited.insert(oid)

    while !queue.isEmpty {
        let current = queue.removeFirst()
        let commit = try readCommit(gitDir: gitDir, oid: current)
        for parentId in commit.parentIds {
            if visited.insert(parentId).inserted {
                queue.append(parentId)
            }
        }
    }

    return visited
}

/// Find the merge base (lowest common ancestor) of two commits.
///
/// Returns the best common ancestor — one that is not an ancestor of any
/// other common ancestor. Returns `nil` if the commits share no history.
public func mergeBase(gitDir: String, oid1: OID, oid2: OID) throws -> OID? {
    if oid1 == oid2 {
        return oid1
    }

    let ancestors1 = try ancestors(gitDir: gitDir, oid: oid1)

    var common: [OID] = []
    var visited = Set<OID>()
    var queue: [OID] = [oid2]
    visited.insert(oid2)

    while !queue.isEmpty {
        let current = queue.removeFirst()
        if ancestors1.contains(current) {
            common.append(current)
            continue
        }
        let commit = try readCommit(gitDir: gitDir, oid: current)
        for parentId in commit.parentIds {
            if visited.insert(parentId).inserted {
                queue.append(parentId)
            }
        }
    }

    if common.isEmpty {
        return nil
    }

    if common.count == 1 {
        return common[0]
    }

    // Filter: remove any common ancestor that is an ancestor of another
    var best = common
    for ca in common {
        let caAncestors = try ancestors(gitDir: gitDir, oid: ca)
        best = best.filter { $0 == ca || !caAncestors.contains($0) }
    }

    return best.first
}

/// Find all merge bases between two commits.
/// In simple cases this returns one OID; for criss-cross merges it may return multiple.
public func mergeBases(gitDir: String, oid1: OID, oid2: OID) throws -> [OID] {
    if oid1 == oid2 {
        return [oid1]
    }

    let ancestors1 = try ancestors(gitDir: gitDir, oid: oid1)

    var common: [OID] = []
    var visited = Set<OID>()
    var queue: [OID] = [oid2]
    visited.insert(oid2)

    while !queue.isEmpty {
        let current = queue.removeFirst()
        if ancestors1.contains(current) {
            common.append(current)
            continue
        }
        let commit = try readCommit(gitDir: gitDir, oid: current)
        for parentId in commit.parentIds {
            if visited.insert(parentId).inserted {
                queue.append(parentId)
            }
        }
    }

    var best = common
    for ca in common {
        let caAncestors = try ancestors(gitDir: gitDir, oid: ca)
        best = best.filter { $0 == ca || !caAncestors.contains($0) }
    }

    return best
}
