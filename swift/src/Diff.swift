/// MuonGit - Tree-to-tree diff
/// Parity: libgit2 src/libgit2/diff.c
import Foundation

/// The kind of change for a diff entry
public enum DiffStatus: Sendable {
    case added
    case deleted
    case modified
}

/// A single diff delta between two trees
public struct DiffDelta: Sendable {
    public let status: DiffStatus
    public let oldEntry: TreeEntry?
    public let newEntry: TreeEntry?
    public let path: String
}

/// Compute the diff between two trees.
/// Both entry lists should be sorted by name (as git trees are).
public func diffTrees(oldEntries: [TreeEntry], newEntries: [TreeEntry]) -> [DiffDelta] {
    var deltas: [DiffDelta] = []
    var oi = 0
    var ni = 0

    while oi < oldEntries.count && ni < newEntries.count {
        let old = oldEntries[oi]
        let new = newEntries[ni]

        if old.name < new.name {
            deltas.append(DiffDelta(status: .deleted, oldEntry: old, newEntry: nil, path: old.name))
            oi += 1
        } else if old.name > new.name {
            deltas.append(DiffDelta(status: .added, oldEntry: nil, newEntry: new, path: new.name))
            ni += 1
        } else {
            if old.oid != new.oid || old.mode != new.mode {
                deltas.append(DiffDelta(status: .modified, oldEntry: old, newEntry: new, path: old.name))
            }
            oi += 1
            ni += 1
        }
    }

    while oi < oldEntries.count {
        let old = oldEntries[oi]
        deltas.append(DiffDelta(status: .deleted, oldEntry: old, newEntry: nil, path: old.name))
        oi += 1
    }

    while ni < newEntries.count {
        let new = newEntries[ni]
        deltas.append(DiffDelta(status: .added, oldEntry: nil, newEntry: new, path: new.name))
        ni += 1
    }

    return deltas
}
