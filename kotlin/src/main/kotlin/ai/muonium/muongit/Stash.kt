// Stash.kt - Git stash save/apply/pop/list/drop
// Parity: libgit2 src/libgit2/stash.c

package ai.muonium.muongit

import java.io.File

/** Stash flags controlling what gets stashed. Parity: git_stash_flags */
enum class StashFlags {
    DEFAULT,
    KEEP_INDEX,
    INCLUDE_UNTRACKED
}

/** A stash entry from the reflog. */
data class StashEntry(
    val index: Int,
    val message: String,
    val oid: OID
)

/** Result of applying a stash. */
data class StashApplyResult(
    val hasConflicts: Boolean,
    val files: List<Triple<String, String, Boolean>>
)

/**
 * Save the current working directory state as a stash entry.
 *
 * Creates the multi-parent stash commit structure:
 * - w_commit (refs/stash target): tree = workdir state, parents = [HEAD, i_commit]
 * - i_commit: tree = index state, parent = HEAD
 *
 * Parity: git_stash_save
 */
fun stashSave(
    gitDir: File,
    workdir: File?,
    stasher: Signature,
    message: String? = null
): OID {
    val wd = workdir ?: throw MuonGitException.BareRepo()

    val headOid = resolveReference(gitDir, "HEAD")

    // Read HEAD commit for branch info
    val (_, headData) = readLooseObject(gitDir, headOid)
    val headCommit = parseCommit(headOid, headData)
    val shortSha = headOid.hex.take(7)

    // Get branch name
    val branch = try {
        val headRef = readReference(gitDir, "HEAD")
        if (headRef.startsWith("ref: refs/heads/")) {
            headRef.removePrefix("ref: refs/heads/")
        } else "(no branch)"
    } catch (_: Exception) {
        "(no branch)"
    }

    val summary = headCommit.message.lines().firstOrNull() ?: ""

    // Collect workdir entries
    val workdirEntries = collectWorkdirEntries(gitDir, wd)
    if (workdirEntries.isEmpty()) {
        throw MuonGitException.NotFound("no local changes to save")
    }

    // Create workdir tree
    val workdirTreeData = serializeTree(workdirEntries)
    val workdirTreeOid = writeLooseObject(gitDir, ObjectType.TREE, workdirTreeData)

    // Create i_commit (index snapshot)
    val iMsg = "index on $branch: $shortSha $summary\n"
    val iData = serializeCommit(
        treeId = workdirTreeOid,
        parentIds = listOf(headOid),
        author = stasher,
        committer = stasher,
        message = iMsg
    )
    val iOid = writeLooseObject(gitDir, ObjectType.COMMIT, iData)

    // Create w_commit (working directory snapshot)
    val stashMsg = if (message != null) {
        "On $branch: $message\n"
    } else {
        "WIP on $branch: $shortSha $summary\n"
    }
    val wData = serializeCommit(
        treeId = workdirTreeOid,
        parentIds = listOf(headOid, iOid),
        author = stasher,
        committer = stasher,
        message = stashMsg
    )
    val wOid = writeLooseObject(gitDir, ObjectType.COMMIT, wData)

    // Update refs/stash
    val oldStash = try { resolveReference(gitDir, "refs/stash") } catch (_: Exception) { OID.ZERO }
    writeReference(gitDir, "refs/stash", wOid)

    // Append to reflog
    val reflogMsg = stashMsg.trim()
    appendReflog(gitDir, "refs/stash", oldStash, wOid, stasher, reflogMsg)

    return wOid
}

/**
 * List all stash entries.
 * Returns entries in reverse order (most recent first, index 0 = newest).
 * Parity: git_stash_foreach
 */
fun stashList(gitDir: File): List<StashEntry> {
    val entries = readReflog(gitDir, "refs/stash")
    val count = entries.size
    return entries.mapIndexed { i, entry ->
        StashEntry(index = count - 1 - i, message = entry.message, oid = entry.newOid)
    }.reversed()
}

/**
 * Apply a stash entry without removing it.
 * Parity: git_stash_apply
 */
fun stashApply(gitDir: File, index: Int = 0): StashApplyResult {
    val entries = readReflog(gitDir, "refs/stash")
    val count = entries.size
    if (index >= count) {
        throw MuonGitException.NotFound("stash@{$index} not found")
    }

    val reflogIdx = count - 1 - index
    val stashOid = entries[reflogIdx].newOid

    return applyStashOid(gitDir, stashOid)
}

/**
 * Pop the stash at position `index`: apply then drop.
 * Parity: git_stash_pop
 */
fun stashPop(gitDir: File, index: Int = 0): StashApplyResult {
    val result = stashApply(gitDir, index)
    if (!result.hasConflicts) {
        stashDrop(gitDir, index)
    }
    return result
}

/**
 * Drop a stash entry by index.
 * Parity: git_stash_drop
 */
fun stashDrop(gitDir: File, index: Int) {
    val entries = readReflog(gitDir, "refs/stash")
    val count = entries.size
    if (index >= count) {
        throw MuonGitException.NotFound("stash@{$index} not found")
    }

    val reflogIdx = count - 1 - index
    val remaining = dropReflogEntry(gitDir, "refs/stash", reflogIdx)

    if (remaining.isEmpty()) {
        deleteReference(gitDir, "refs/stash")
    } else {
        val newest = remaining.last()
        writeReference(gitDir, "refs/stash", newest.newOid)
    }
}

/** Drop a reflog entry by index. Returns remaining entries. */
internal fun dropReflogEntry(gitDir: File, refName: String, index: Int): List<ReflogEntry> {
    val logFile = File(gitDir, "logs/$refName")
    val entries = readReflog(gitDir, refName).toMutableList()

    if (index >= entries.size) {
        throw MuonGitException.NotFound("reflog entry $index not found for $refName")
    }

    entries.removeAt(index)

    if (entries.isEmpty()) {
        logFile.delete()
    } else {
        val content = entries.joinToString("") { entry ->
            formatReflogEntry(entry.oldOid, entry.newOid, entry.committer, entry.message)
        }
        logFile.writeText(content)
    }

    return entries
}

// ── Internal helpers ──

/** Collect workdir files as tree entries (single-level, skipping .git). */
private fun collectWorkdirEntries(gitDir: File, workdir: File): List<TreeEntry> {
    if (!workdir.isDirectory) return emptyList()

    val entries = mutableListOf<TreeEntry>()
    val files = workdir.listFiles() ?: return emptyList()

    for (file in files) {
        if (file.name == ".git") continue
        if (!file.isFile) continue

        val data = file.readBytes()
        val blobOid = writeLooseObject(gitDir, ObjectType.BLOB, data)
        entries.add(TreeEntry(mode = FileMode.BLOB, name = file.name, oid = blobOid))
    }

    entries.sortBy { it.name }
    return entries
}

/** Apply a stash commit by OID. */
private fun applyStashOid(gitDir: File, stashOid: OID): StashApplyResult {
    val (objType, data) = readLooseObject(gitDir, stashOid)
    if (objType != ObjectType.COMMIT) {
        throw MuonGitException.InvalidObject("stash is not a commit")
    }
    val wCommit = parseCommit(stashOid, data)

    if (wCommit.parentIds.isEmpty()) {
        throw MuonGitException.InvalidObject("stash commit has no parents")
    }
    val baseOid = wCommit.parentIds[0]

    val baseEntries = loadCommitTree(gitDir, baseOid)
    val stashEntries = loadTreeEntries(gitDir, wCommit.treeId)

    val headOid = resolveReference(gitDir, "HEAD")
    val headEntries = loadCommitTree(gitDir, headOid)

    val mergeResult = mergeTreesContent(gitDir, baseEntries, headEntries, stashEntries)

    return StashApplyResult(hasConflicts = mergeResult.hasConflicts, files = mergeResult.files)
}


