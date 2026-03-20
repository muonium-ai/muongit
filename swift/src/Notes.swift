/// MuonGit - Git notes: metadata annotations on commits
/// Parity: libgit2 src/libgit2/notes.c
import Foundation

/// Default notes reference
public let defaultNotesRef = "refs/notes/commits"

/// A git note attached to an object
public struct Note: Sendable {
    public let noteOid: OID
    public let annotatedOid: OID
    public let message: String
}

/// Read a note for a specific object
public func noteRead(gitDir: String, notesRef: String? = nil, targetOid: OID) throws -> Note {
    let ref = notesRef ?? defaultNotesRef

    let notesCommitOid = try resolveReference(gitDir: gitDir, name: ref)
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: notesCommitOid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("notes ref not a commit")
    }
    let commit = try parseCommit(oid: notesCommitOid, data: data)

    let noteOid = try findNoteInTree(gitDir: gitDir, treeOid: commit.treeId, targetHex: targetOid.hex)

    let (blobType, blobData) = try readLooseObject(gitDir: gitDir, oid: noteOid)
    guard blobType == .blob else {
        throw MuonGitError.invalidObject("note is not a blob")
    }
    guard let message = String(data: blobData, encoding: .utf8) else {
        throw MuonGitError.invalidObject("note is not valid UTF-8")
    }

    return Note(noteOid: noteOid, annotatedOid: targetOid, message: message)
}

/// List all notes under a notes ref
public func noteList(gitDir: String, notesRef: String? = nil) throws -> [(noteOid: OID, annotatedOid: OID)] {
    let ref = notesRef ?? defaultNotesRef

    let notesCommitOid = try resolveReference(gitDir: gitDir, name: ref)
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: notesCommitOid)
    guard objType == .commit else {
        throw MuonGitError.invalidObject("notes ref not a commit")
    }
    let commit = try parseCommit(oid: notesCommitOid, data: data)

    var notes: [(OID, OID)] = []
    try collectNotesFromTree(gitDir: gitDir, treeOid: commit.treeId, prefix: "", notes: &notes)
    return notes
}

// MARK: - Internal

private func findNoteInTree(gitDir: String, treeOid: OID, targetHex: String) throws -> OID {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: treeOid)
    guard objType == .tree else {
        throw MuonGitError.invalidObject("expected tree")
    }
    let tree = try parseTree(oid: treeOid, data: data)

    if targetHex.count >= 2 {
        let prefix = String(targetHex.prefix(2))
        let rest = String(targetHex.dropFirst(2))

        for entry in tree.entries {
            if entry.name == prefix && entry.mode == 0o040000 {
                return try findNoteInTree(gitDir: gitDir, treeOid: entry.oid, targetHex: rest)
            }
        }
    }

    for entry in tree.entries {
        if entry.name == targetHex {
            return entry.oid
        }
    }

    throw MuonGitError.notFound("no note found for \(targetHex)")
}

private func collectNotesFromTree(gitDir: String, treeOid: OID, prefix: String, notes: inout [(OID, OID)]) throws {
    let (objType, data) = try readLooseObject(gitDir: gitDir, oid: treeOid)
    guard objType == .tree else { return }
    let tree = try parseTree(oid: treeOid, data: data)

    for entry in tree.entries {
        if entry.mode == 0o040000 {
            let newPrefix = "\(prefix)\(entry.name)"
            try collectNotesFromTree(gitDir: gitDir, treeOid: entry.oid, prefix: newPrefix, notes: &notes)
        } else {
            let fullHex = "\(prefix)\(entry.name)"
            if fullHex.count == 40 {
                let annotatedOid = OID(hex: fullHex)
                notes.append((entry.oid, annotatedOid))
            }
        }
    }
}
