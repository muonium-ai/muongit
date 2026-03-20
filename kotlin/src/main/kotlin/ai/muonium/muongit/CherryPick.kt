package ai.muonium.muongit

import java.io.File

/** Options for cherry-pick */
data class CherryPickOptions(
    /** For merge commits, which parent to diff against (1-based, default 1) */
    val mainline: Int = 1
)

/** Result of a cherry-pick operation */
data class CherryPickResult(
    val hasConflicts: Boolean,
    /** Merged file contents: (path, content, conflicted) */
    val files: List<Triple<String, String, Boolean>>,
    val cherryPickedCommit: OID
)

/** Result of merging tree contents (shared with Revert and Rebase) */
data class TreeMergeResult(
    val hasConflicts: Boolean,
    val files: List<Triple<String, String, Boolean>>
)

/** Cherry-pick a commit onto HEAD.
 *  Three-way merge: merge(parent_tree, head_tree, commit_tree) */
fun cherryPick(
    gitDir: File,
    commitOid: OID,
    options: CherryPickOptions = CherryPickOptions()
): CherryPickResult {
    val (objType, data) = readLooseObject(gitDir, commitOid)
    if (objType != ObjectType.COMMIT) throw MuonGitException.InvalidObject("not a commit")
    val commit = parseCommit(commitOid, data)

    if (commit.parentIds.isEmpty()) throw MuonGitException.InvalidObject("cannot cherry-pick a root commit")
    val parentIdx = (options.mainline - 1).coerceAtLeast(0)
    if (parentIdx >= commit.parentIds.size) throw MuonGitException.InvalidObject("mainline parent not found")
    val parentOid = commit.parentIds[parentIdx]

    val parentTree = loadCommitTree(gitDir, parentOid)
    val commitTree = loadCommitTreeDirect(gitDir, commit)
    val headOid = resolveReference(gitDir, "HEAD")
    val headTree = loadCommitTree(gitDir, headOid)

    val result = mergeTreesContent(gitDir, parentTree, headTree, commitTree)

    // Write state files
    gitDir.resolve("CHERRY_PICK_HEAD").writeText(commitOid.hex)
    gitDir.resolve("MERGE_MSG").writeText(commit.message)

    return CherryPickResult(
        hasConflicts = result.hasConflicts,
        files = result.files,
        cherryPickedCommit = commitOid
    )
}

/** Clean up cherry-pick state files */
fun cherryPickCleanup(gitDir: File) {
    gitDir.resolve("CHERRY_PICK_HEAD").delete()
    gitDir.resolve("MERGE_MSG").delete()
}

// -- Shared helpers --

internal fun loadCommitTree(gitDir: File, commitOid: OID): List<TreeEntry> {
    val (objType, data) = readLooseObject(gitDir, commitOid)
    if (objType != ObjectType.COMMIT) throw MuonGitException.InvalidObject("expected commit")
    val commit = parseCommit(commitOid, data)
    return loadTreeEntries(gitDir, commit.treeId)
}

internal fun loadCommitTreeDirect(gitDir: File, commit: Commit): List<TreeEntry> {
    return loadTreeEntries(gitDir, commit.treeId)
}

internal fun loadTreeEntries(gitDir: File, treeOid: OID): List<TreeEntry> {
    val (objType, data) = readLooseObject(gitDir, treeOid)
    if (objType != ObjectType.TREE) throw MuonGitException.InvalidObject("expected tree")
    return parseTree(treeOid, data).entries
}

internal fun readBlobText(gitDir: File, oid: OID): String {
    if (oid.isZero) return ""
    return try {
        val (objType, data) = readLooseObject(gitDir, oid)
        if (objType == ObjectType.BLOB) String(data, Charsets.UTF_8) else ""
    } catch (_: Exception) { "" }
}

/** Merge two trees against a base, producing per-file merge results. */
fun mergeTreesContent(
    gitDir: File,
    base: List<TreeEntry>,
    ours: List<TreeEntry>,
    theirs: List<TreeEntry>
): TreeMergeResult {
    val allPaths = sortedMapOf<String, Triple<OID?, OID?, OID?>>()
    for (e in base) {
        val cur = allPaths.getOrDefault(e.name, Triple(null, null, null))
        allPaths[e.name] = cur.copy(first = e.oid)
    }
    for (e in ours) {
        val cur = allPaths.getOrDefault(e.name, Triple(null, null, null))
        allPaths[e.name] = cur.copy(second = e.oid)
    }
    for (e in theirs) {
        val cur = allPaths.getOrDefault(e.name, Triple(null, null, null))
        allPaths[e.name] = cur.copy(third = e.oid)
    }

    val files = mutableListOf<Triple<String, String, Boolean>>()
    var hasConflicts = false
    val zero = OID.ZERO

    for ((path, value) in allPaths) {
        val b = value.first ?: zero
        val o = value.second ?: zero
        val t = value.third ?: zero

        if (o == t) {
            files.add(Triple(path, readBlobText(gitDir, o), false))
            continue
        }
        if (o == b) {
            if (t.isZero) continue
            files.add(Triple(path, readBlobText(gitDir, t), false))
            continue
        }
        if (t == b) {
            if (o.isZero) continue
            files.add(Triple(path, readBlobText(gitDir, o), false))
            continue
        }

        // Both sides changed
        val baseText = readBlobText(gitDir, b)
        val oursText = readBlobText(gitDir, o)
        val theirsText = readBlobText(gitDir, t)

        val mergeResult = merge3(baseText, oursText, theirsText)
        if (mergeResult.hasConflicts) {
            hasConflicts = true
            files.add(Triple(path, mergeResult.toStringWithMarkers(), true))
        } else {
            files.add(Triple(path, mergeResult.toCleanString() ?: "", false))
        }
    }

    return TreeMergeResult(hasConflicts, files)
}
