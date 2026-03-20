package ai.muonium.muongit

import java.io.File

/** Options for revert */
data class RevertOptions(
    /** For merge commits, which parent to use (1-based, default 1) */
    val mainline: Int = 1
)

/** Result of a revert operation */
data class RevertResult(
    val hasConflicts: Boolean,
    /** Merged file contents: (path, content, conflicted) */
    val files: List<Triple<String, String, Boolean>>,
    val revertedCommit: OID
)

/** Revert a commit against HEAD.
 *  Three-way merge with swapped args: merge(commit_tree, head_tree, parent_tree) */
fun revert(
    gitDir: File,
    commitOid: OID,
    options: RevertOptions = RevertOptions()
): RevertResult {
    val (objType, data) = readLooseObject(gitDir, commitOid)
    if (objType != ObjectType.COMMIT) throw MuonGitException.InvalidObject("not a commit")
    val commit = parseCommit(commitOid, data)

    if (commit.parentIds.isEmpty()) throw MuonGitException.InvalidObject("cannot revert a root commit")
    val parentIdx = (options.mainline - 1).coerceAtLeast(0)
    if (parentIdx >= commit.parentIds.size) throw MuonGitException.InvalidObject("mainline parent not found")
    val parentOid = commit.parentIds[parentIdx]

    // Revert swaps base and theirs
    val commitTree = loadCommitTreeDirect(gitDir, commit)
    val headOid = resolveReference(gitDir, "HEAD")
    val headTree = loadCommitTree(gitDir, headOid)
    val parentTree = loadCommitTree(gitDir, parentOid)

    val result = mergeTreesContent(gitDir, commitTree, headTree, parentTree)

    // Write state files
    gitDir.resolve("REVERT_HEAD").writeText(commitOid.hex)
    val revertMsg = "Revert \"${commit.message.trim()}\"\n\nThis reverts commit ${commitOid.hex}.\n"
    gitDir.resolve("MERGE_MSG").writeText(revertMsg)

    return RevertResult(
        hasConflicts = result.hasConflicts,
        files = result.files,
        revertedCommit = commitOid
    )
}

/** Clean up revert state files */
fun revertCleanup(gitDir: File) {
    gitDir.resolve("REVERT_HEAD").delete()
    gitDir.resolve("MERGE_MSG").delete()
}
