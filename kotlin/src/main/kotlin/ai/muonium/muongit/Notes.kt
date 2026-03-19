package ai.muonium.muongit

import java.io.File

/** Default notes reference */
const val DEFAULT_NOTES_REF = "refs/notes/commits"

/** A git note attached to an object */
data class Note(
    val noteOid: OID,
    val annotatedOid: OID,
    val message: String
)

/** Read a note for a specific object */
fun noteRead(gitDir: File, targetOid: OID, notesRef: String = DEFAULT_NOTES_REF): Note {
    val notesCommitOid = resolveReference(gitDir, notesRef)
    val (objType, data) = readLooseObject(gitDir, notesCommitOid)
    if (objType != ObjectType.COMMIT) throw MuonGitException.InvalidObject("notes ref not a commit")
    val commit = parseCommit(notesCommitOid, data)

    val noteOid = findNoteInTree(gitDir, commit.treeId, targetOid.hex)

    val (blobType, blobData) = readLooseObject(gitDir, noteOid)
    if (blobType != ObjectType.BLOB) throw MuonGitException.InvalidObject("note is not a blob")

    return Note(
        noteOid = noteOid,
        annotatedOid = targetOid,
        message = blobData.decodeToString()
    )
}

/** List all notes under a notes ref */
fun noteList(gitDir: File, notesRef: String = DEFAULT_NOTES_REF): List<Pair<OID, OID>> {
    val notesCommitOid = resolveReference(gitDir, notesRef)
    val (objType, data) = readLooseObject(gitDir, notesCommitOid)
    if (objType != ObjectType.COMMIT) throw MuonGitException.InvalidObject("notes ref not a commit")
    val commit = parseCommit(notesCommitOid, data)

    val notes = mutableListOf<Pair<OID, OID>>()
    collectNotesFromTree(gitDir, commit.treeId, "", notes)
    return notes
}

private fun findNoteInTree(gitDir: File, treeOid: OID, targetHex: String): OID {
    val (objType, data) = readLooseObject(gitDir, treeOid)
    if (objType != ObjectType.TREE) throw MuonGitException.InvalidObject("expected tree")
    val tree = parseTree(treeOid, data)

    if (targetHex.length >= 2) {
        val prefix = targetHex.substring(0, 2)
        val rest = targetHex.substring(2)

        for (entry in tree.entries) {
            if (entry.name == prefix && entry.mode == 0x4000) {
                return findNoteInTree(gitDir, entry.oid, rest)
            }
            if (entry.name == targetHex) return entry.oid
        }
    }

    for (entry in tree.entries) {
        if (entry.name == targetHex) return entry.oid
    }

    throw MuonGitException.NotFound("no note found for $targetHex")
}

private fun collectNotesFromTree(gitDir: File, treeOid: OID, prefix: String, notes: MutableList<Pair<OID, OID>>) {
    val (objType, data) = try {
        readLooseObject(gitDir, treeOid)
    } catch (_: Exception) { return }
    if (objType != ObjectType.TREE) return
    val tree = parseTree(treeOid, data)

    for (entry in tree.entries) {
        if (entry.mode == 0x4000) {
            collectNotesFromTree(gitDir, entry.oid, prefix + entry.name, notes)
        } else {
            val fullHex = prefix + entry.name
            if (fullHex.length == 40 && fullHex.all { it in '0'..'9' || it in 'a'..'f' }) {
                notes.add(entry.oid to OID(fullHex))
            }
        }
    }
}
