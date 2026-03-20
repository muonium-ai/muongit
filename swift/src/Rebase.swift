/// MuonGit - Rebase support
/// Parity: libgit2 src/libgit2/rebase.c
import Foundation

/// Type of rebase operation
public enum RebaseOperationType: Sendable {
    case pick
}

/// A single rebase operation
public struct RebaseOperation: Sendable {
    public let opType: RebaseOperationType
    public let id: OID
}

/// Options for rebase
public struct RebaseOptions: Sendable {
    public var inmemory: Bool
    public init(inmemory: Bool = false) { self.inmemory = inmemory }
}

/// An in-progress rebase
public class Rebase {
    public let gitDir: String
    public private(set) var operations: [RebaseOperation]
    public private(set) var current: Int?
    public let ontoId: OID
    public let origHeadId: OID
    public let origHeadName: String
    public private(set) var lastCommitId: OID?
    public let inmemory: Bool

    private init(
        gitDir: String,
        operations: [RebaseOperation],
        current: Int?,
        ontoId: OID,
        origHeadId: OID,
        origHeadName: String,
        lastCommitId: OID?,
        inmemory: Bool
    ) {
        self.gitDir = gitDir
        self.operations = operations
        self.current = current
        self.ontoId = ontoId
        self.origHeadId = origHeadId
        self.origHeadName = origHeadName
        self.lastCommitId = lastCommitId
        self.inmemory = inmemory
    }

    /// Start a new rebase.
    /// Replays all commits from `branch` not in `upstream` onto `onto`.
    public static func begin(
        gitDir: String,
        branch: OID,
        upstream: OID,
        onto: OID? = nil,
        options: RebaseOptions = RebaseOptions()
    ) throws -> Rebase {
        let ontoId = onto ?? upstream

        let commits = try collectCommitsToRebase(gitDir: gitDir, branch: branch, upstream: upstream)
        guard !commits.isEmpty else {
            throw MuonGitError.notFound("nothing to rebase")
        }

        let operations = commits.map { RebaseOperation(opType: .pick, id: $0) }

        let headContent = try String(contentsOfFile: gitDir + "/HEAD", encoding: .utf8)
        let origHeadName = headContent.trimmingCharacters(in: .whitespacesAndNewlines)

        if !options.inmemory {
            let stateDir = gitDir + "/rebase-merge"
            let fm = FileManager.default
            try fm.createDirectory(atPath: stateDir, withIntermediateDirectories: true)
            try origHeadName.write(toFile: stateDir + "/head-name", atomically: true, encoding: .utf8)
            try branch.hex.write(toFile: stateDir + "/orig-head", atomically: true, encoding: .utf8)
            try ontoId.hex.write(toFile: stateDir + "/onto", atomically: true, encoding: .utf8)
            try String(operations.count).write(toFile: stateDir + "/end", atomically: true, encoding: .utf8)
            try "0".write(toFile: stateDir + "/msgnum", atomically: true, encoding: .utf8)

            for (i, op) in operations.enumerated() {
                try op.id.hex.write(toFile: stateDir + "/cmt.\(i + 1)", atomically: true, encoding: .utf8)
            }
        }

        return Rebase(
            gitDir: gitDir,
            operations: operations,
            current: nil,
            ontoId: ontoId,
            origHeadId: branch,
            origHeadName: origHeadName,
            lastCommitId: ontoId,
            inmemory: options.inmemory
        )
    }

    /// Open an existing rebase in progress
    public static func open(gitDir: String) throws -> Rebase {
        let stateDir = gitDir + "/rebase-merge"
        let fm = FileManager.default
        guard fm.fileExists(atPath: stateDir) else {
            throw MuonGitError.notFound("no rebase in progress")
        }

        let origHeadName = try String(contentsOfFile: stateDir + "/head-name", encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let origHeadHex = try String(contentsOfFile: stateDir + "/orig-head", encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let ontoHex = try String(contentsOfFile: stateDir + "/onto", encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let end = Int(try String(contentsOfFile: stateDir + "/end", encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines))!
        let msgnum = Int(try String(contentsOfFile: stateDir + "/msgnum", encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines))!

        var operations: [RebaseOperation] = []
        for i in 1...end {
            let hex = try String(contentsOfFile: stateDir + "/cmt.\(i)", encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
            operations.append(RebaseOperation(opType: .pick, id: OID(hex: hex)))
        }

        let current = msgnum > 0 ? msgnum - 1 : nil

        return Rebase(
            gitDir: gitDir,
            operations: operations,
            current: current,
            ontoId: OID(hex: ontoHex),
            origHeadId: OID(hex: origHeadHex),
            origHeadName: origHeadName,
            lastCommitId: nil,
            inmemory: false
        )
    }

    /// Get the next operation. Returns nil when all done.
    public func next() throws -> RebaseOperation? {
        let nextIdx: Int
        if let c = current { nextIdx = c + 1 } else { nextIdx = 0 }

        guard nextIdx < operations.count else { return nil }
        current = nextIdx

        if !inmemory {
            let stateDir = gitDir + "/rebase-merge"
            try String(nextIdx + 1).write(toFile: stateDir + "/msgnum", atomically: true, encoding: .utf8)
        }

        return operations[nextIdx]
    }

    /// Apply the current operation (cherry-pick onto current base)
    public func applyCurrent() throws -> (hasConflicts: Bool, files: [(String, String, Bool)]) {
        guard let idx = current else {
            throw MuonGitError.notFound("no current rebase operation")
        }

        let op = operations[idx]
        let (objType, data) = try readLooseObject(gitDir: gitDir, oid: op.id)
        guard objType == .commit else {
            throw MuonGitError.invalidObject("not a commit")
        }
        let commit = try parseCommit(oid: op.id, data: data)

        guard !commit.parentIds.isEmpty else {
            throw MuonGitError.invalidObject("cannot rebase a root commit")
        }

        let parentTree = try loadCommitTree(gitDir: gitDir, commitOid: commit.parentIds[0])
        let ontoTip = lastCommitId ?? ontoId
        let oursTree = try loadCommitTree(gitDir: gitDir, commitOid: ontoTip)
        let theirsTree = try loadCommitTreeDirect(gitDir: gitDir, commit: commit)

        let result = try mergeTreesContent(gitDir: gitDir, base: parentTree, ours: oursTree, theirs: theirsTree)
        return (result.hasConflicts, result.files)
    }

    /// Commit the current operation's result
    public func commit(
        author: Signature? = nil,
        committer: Signature,
        message: String? = nil
    ) throws -> OID {
        guard let idx = current else {
            throw MuonGitError.notFound("no current rebase operation")
        }

        let op = operations[idx]
        let (_, data) = try readLooseObject(gitDir: gitDir, oid: op.id)
        let origCommit = try parseCommit(oid: op.id, data: data)

        let actualAuthor = author ?? origCommit.author
        let actualMessage = message ?? origCommit.message

        let (hasConflicts, files) = try applyCurrent()
        if hasConflicts {
            throw MuonGitError.conflict("cannot commit with conflicts")
        }

        // Build new tree
        var entries: [TreeEntry] = []
        for (path, content, _) in files {
            let blobOid = try writeLooseObject(gitDir: gitDir, type: .blob, data: Data(content.utf8))
            entries.append(TreeEntry(mode: FileMode.blob.rawValue, name: path, oid: blobOid))
        }
        let treeData = serializeTree(entries: entries)
        let treeOid = try writeLooseObject(gitDir: gitDir, type: .tree, data: treeData)

        let parent = lastCommitId ?? ontoId
        let commitData = serializeCommit(
            treeId: treeOid,
            parentIds: [parent],
            author: actualAuthor,
            committer: committer,
            message: actualMessage,
            messageEncoding: origCommit.messageEncoding
        )
        let newOid = try writeLooseObject(gitDir: gitDir, type: .commit, data: commitData)

        lastCommitId = newOid
        return newOid
    }

    /// Abort the rebase and restore original state
    public func abort() throws {
        if !inmemory {
            try origHeadName.appending("\n").write(toFile: gitDir + "/HEAD", atomically: true, encoding: .utf8)
            if origHeadName.hasPrefix("ref: ") {
                let refName = String(origHeadName.dropFirst(5))
                try writeReference(gitDir: gitDir, name: refName, oid: origHeadId)
            }
            try? FileManager.default.removeItem(atPath: gitDir + "/rebase-merge")
        }
    }

    /// Finish the rebase — update branch ref, clean up
    public func finish() throws {
        if !inmemory {
            if let newHead = lastCommitId, origHeadName.hasPrefix("ref: ") {
                let refName = String(origHeadName.dropFirst(5))
                try writeReference(gitDir: gitDir, name: refName, oid: newHead)
            }
            try? FileManager.default.removeItem(atPath: gitDir + "/rebase-merge")
        }
    }

    public var operationCount: Int { operations.count }
}

// MARK: - Private helpers

private func collectCommitsToRebase(gitDir: String, branch: OID, upstream: OID) throws -> [OID] {
    var commits: [OID] = []
    var current = branch

    for _ in 0..<10000 {
        if current == upstream { break }

        guard let (objType, data) = try? readLooseObject(gitDir: gitDir, oid: current),
              objType == .commit else { break }
        let commit = try parseCommit(oid: current, data: data)

        commits.append(current)

        guard let parent = commit.parentIds.first else { break }
        current = parent
    }

    commits.reverse()
    return commits
}
