package ai.muonium.muongit

import java.io.File

/** Type of rebase operation */
enum class RebaseOperationType { PICK }

/** A single rebase operation */
data class RebaseOperation(
    val opType: RebaseOperationType,
    val id: OID
)

/** Options for rebase */
data class RebaseOptions(
    val inmemory: Boolean = false
)

/** An in-progress rebase */
class Rebase private constructor(
    val gitDir: File,
    val operations: List<RebaseOperation>,
    private var _current: Int?,
    val ontoId: OID,
    val origHeadId: OID,
    val origHeadName: String,
    private var _lastCommitId: OID?,
    val inmemory: Boolean
) {
    val current: Int? get() = _current
    val lastCommitId: OID? get() = _lastCommitId
    val operationCount: Int get() = operations.size

    companion object {
        /** Start a new rebase. Replays commits from branch not in upstream onto onto. */
        fun begin(
            gitDir: File,
            branch: OID,
            upstream: OID,
            onto: OID? = null,
            options: RebaseOptions = RebaseOptions()
        ): Rebase {
            val ontoId = onto ?: upstream
            val commits = collectCommitsToRebase(gitDir, branch, upstream)
            if (commits.isEmpty()) throw MuonGitException.NotFound("nothing to rebase")

            val operations = commits.map { RebaseOperation(RebaseOperationType.PICK, it) }

            val headContent = gitDir.resolve("HEAD").readText().trim()

            if (!options.inmemory) {
                val stateDir = gitDir.resolve("rebase-merge")
                stateDir.mkdirs()
                stateDir.resolve("head-name").writeText(headContent)
                stateDir.resolve("orig-head").writeText(branch.hex)
                stateDir.resolve("onto").writeText(ontoId.hex)
                stateDir.resolve("end").writeText(operations.size.toString())
                stateDir.resolve("msgnum").writeText("0")

                operations.forEachIndexed { i, op ->
                    stateDir.resolve("cmt.${i + 1}").writeText(op.id.hex)
                }
            }

            return Rebase(
                gitDir = gitDir,
                operations = operations,
                _current = null,
                ontoId = ontoId,
                origHeadId = branch,
                origHeadName = headContent,
                _lastCommitId = ontoId,
                inmemory = options.inmemory
            )
        }

        /** Open an existing rebase in progress */
        fun open(gitDir: File): Rebase {
            val stateDir = gitDir.resolve("rebase-merge")
            if (!stateDir.exists()) throw MuonGitException.NotFound("no rebase in progress")

            val origHeadName = stateDir.resolve("head-name").readText().trim()
            val origHeadHex = stateDir.resolve("orig-head").readText().trim()
            val ontoHex = stateDir.resolve("onto").readText().trim()
            val end = stateDir.resolve("end").readText().trim().toInt()
            val msgnum = stateDir.resolve("msgnum").readText().trim().toInt()

            val operations = (1..end).map { i ->
                val hex = stateDir.resolve("cmt.$i").readText().trim()
                RebaseOperation(RebaseOperationType.PICK, OID(hex))
            }

            val current = if (msgnum > 0) msgnum - 1 else null

            return Rebase(
                gitDir = gitDir,
                operations = operations,
                _current = current,
                ontoId = OID(ontoHex),
                origHeadId = OID(origHeadHex),
                origHeadName = origHeadName,
                _lastCommitId = null,
                inmemory = false
            )
        }

        private fun collectCommitsToRebase(gitDir: File, branch: OID, upstream: OID): List<OID> {
            val commits = mutableListOf<OID>()
            var current = branch

            for (i in 0 until 10000) {
                if (current == upstream) break
                val (objType, data) = try { readLooseObject(gitDir, current) } catch (_: Exception) { break }
                if (objType != ObjectType.COMMIT) break
                val commit = parseCommit(current, data)
                commits.add(current)
                current = commit.parentIds.firstOrNull() ?: break
            }

            commits.reverse()
            return commits
        }
    }

    /** Get the next operation. Returns null when all done. */
    fun next(): RebaseOperation? {
        val nextIdx = if (_current != null) _current!! + 1 else 0
        if (nextIdx >= operations.size) return null

        _current = nextIdx

        if (!inmemory) {
            val stateDir = gitDir.resolve("rebase-merge")
            stateDir.resolve("msgnum").writeText((nextIdx + 1).toString())
        }

        return operations[nextIdx]
    }

    /** Apply the current operation (cherry-pick onto current base) */
    fun applyCurrent(): Pair<Boolean, List<Triple<String, String, Boolean>>> {
        val idx = _current ?: throw MuonGitException.NotFound("no current rebase operation")
        val op = operations[idx]

        val (objType, data) = readLooseObject(gitDir, op.id)
        if (objType != ObjectType.COMMIT) throw MuonGitException.InvalidObject("not a commit")
        val commit = parseCommit(op.id, data)

        if (commit.parentIds.isEmpty()) throw MuonGitException.InvalidObject("cannot rebase a root commit")

        val parentTree = loadCommitTree(gitDir, commit.parentIds[0])
        val ontoTip = _lastCommitId ?: ontoId
        val oursTree = loadCommitTree(gitDir, ontoTip)
        val theirsTree = loadCommitTreeDirect(gitDir, commit)

        val result = mergeTreesContent(gitDir, parentTree, oursTree, theirsTree)
        return Pair(result.hasConflicts, result.files)
    }

    /** Commit the current operation's result */
    fun commit(
        author: Signature? = null,
        committer: Signature,
        message: String? = null
    ): OID {
        val idx = _current ?: throw MuonGitException.NotFound("no current rebase operation")
        val op = operations[idx]

        val (_, data) = readLooseObject(gitDir, op.id)
        val origCommit = parseCommit(op.id, data)

        val actualAuthor = author ?: origCommit.author
        val actualMessage = message ?: origCommit.message

        val (hasConflicts, files) = applyCurrent()
        if (hasConflicts) throw MuonGitException.Conflict("cannot commit with conflicts")

        // Build new tree
        val entries = files.map { (path, content, _) ->
            val blobOid = writeLooseObject(gitDir, ObjectType.BLOB, content.toByteArray())
            TreeEntry(mode = FileMode.BLOB, name = path, oid = blobOid)
        }
        val treeData = serializeTree(entries)
        val treeOid = writeLooseObject(gitDir, ObjectType.TREE, treeData)

        val parent = _lastCommitId ?: ontoId
        val commitData = serializeCommit(
            treeId = treeOid,
            parentIds = listOf(parent),
            author = actualAuthor,
            committer = committer,
            message = actualMessage,
            messageEncoding = origCommit.messageEncoding
        )
        val newOid = writeLooseObject(gitDir, ObjectType.COMMIT, commitData)

        _lastCommitId = newOid
        return newOid
    }

    /** Abort the rebase and restore original state */
    fun abort() {
        if (!inmemory) {
            gitDir.resolve("HEAD").writeText(origHeadName + "\n")
            if (origHeadName.startsWith("ref: ")) {
                val refName = origHeadName.removePrefix("ref: ")
                writeReference(gitDir, refName, origHeadId)
            }
            gitDir.resolve("rebase-merge").deleteRecursively()
        }
    }

    /** Finish the rebase — update branch ref, clean up */
    fun finish() {
        if (!inmemory) {
            val newHead = _lastCommitId
            if (newHead != null && origHeadName.startsWith("ref: ")) {
                val refName = origHeadName.removePrefix("ref: ")
                writeReference(gitDir, refName, newHead)
            }
            gitDir.resolve("rebase-merge").deleteRecursively()
        }
    }
}
