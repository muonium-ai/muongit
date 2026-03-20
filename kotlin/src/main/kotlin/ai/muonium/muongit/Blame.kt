// Blame.kt - Line-by-line attribution of file contents to commits
// Parity: libgit2 src/libgit2/blame.c, blame_git.c

package ai.muonium.muongit

import java.io.File

/** Options controlling blame behavior */
data class BlameOptions(
    /** Restrict blame to this commit (newest). Default: HEAD. */
    val newestCommit: OID? = null,
    /** Stop blaming at this commit. Default: root. */
    val oldestCommit: OID? = null,
    /** Only blame lines in [minLine, maxLine] (1-based, inclusive). 0 = all. */
    val minLine: Int = 0,
    val maxLine: Int = 0
)

/** A hunk of lines attributed to a single commit */
data class BlameHunk(
    /** Number of lines in this hunk */
    val linesInHunk: Int,
    /** The commit that introduced these lines */
    val finalCommitId: OID,
    /** 1-based start line in the final file */
    val finalStartLineNumber: Int,
    /** Author signature from the blamed commit */
    val finalSignature: Signature?,
    /** The original commit (same as final unless tracking copies) */
    val origCommitId: OID,
    /** 1-based start line in the original file */
    val origStartLineNumber: Int,
    /** Original path if different from blamed path */
    val origPath: String? = null,
    /** True if this hunk is at the oldest_commit boundary */
    val boundary: Boolean = false
)

/** Result of a blame operation */
data class BlameResult(
    /** The path that was blamed */
    val path: String,
    /** Blame hunks covering all lines */
    val hunks: List<BlameHunk>,
    /** Total line count in the file */
    val lineCount: Int
) {
    /** Number of hunks */
    val hunkCount: Int get() = hunks.size

    /** Get hunk by 0-based index */
    fun hunkByIndex(index: Int): BlameHunk? =
        hunks.getOrNull(index)

    /** Get the hunk that covers a specific 1-based line number */
    fun hunkByLine(line: Int): BlameHunk? {
        if (line < 1 || line > lineCount) return null
        return hunks.firstOrNull { hunk ->
            line >= hunk.finalStartLineNumber &&
                line < hunk.finalStartLineNumber + hunk.linesInHunk
        }
    }
}

/** Blame a file, attributing each line to the commit that last changed it. */
fun blameFile(
    gitDir: File,
    path: String,
    options: BlameOptions? = null
): BlameResult {
    val opts = options ?: BlameOptions()

    // Resolve starting commit
    val startOid = opts.newestCommit ?: resolveReference(gitDir, "HEAD")

    // Read file content at starting commit
    val fileContent = readBlobAtCommit(gitDir, startOid, path)
    val lines = if (fileContent.isEmpty()) emptyList() else fileContent.split("\n")
    val totalLines = lines.size

    if (totalLines == 0) {
        return BlameResult(path = path, hunks = emptyList(), lineCount = 0)
    }

    val minLine = if (opts.minLine > 0) opts.minLine else 1
    val maxLine = if (opts.maxLine > 0) minOf(opts.maxLine, totalLines) else totalLines

    // Per-line tracking: (commitOid, origLine1Based)
    val lineOwners = arrayOfNulls<Pair<OID, Int>>(totalLines)

    var currentOid = startOid
    var currentContent = fileContent
    var remaining = maxLine - minLine + 1
    val maxDepth = 10000

    for (depth in 0 until maxDepth) {
        if (remaining <= 0) break

        val commit = readCommitObj(gitDir, currentOid)

        if (commit.parentIds.isEmpty()) {
            // Root commit — attribute all remaining lines
            for (i in 0 until totalLines) {
                val line1 = i + 1
                if (lineOwners[i] == null && line1 in minLine..maxLine) {
                    lineOwners[i] = Pair(currentOid, line1)
                }
            }
            break
        }

        // Check oldest_commit boundary
        if (opts.oldestCommit != null && currentOid == opts.oldestCommit) {
            for (i in 0 until totalLines) {
                val line1 = i + 1
                if (lineOwners[i] == null && line1 in minLine..maxLine) {
                    lineOwners[i] = Pair(currentOid, line1)
                }
            }
            break
        }

        val parentOid = commit.parentIds[0]

        // Read file at parent
        val parentContent: String
        try {
            parentContent = readBlobAtCommit(gitDir, parentOid, path)
        } catch (_: Exception) {
            // File didn't exist in parent
            for (i in 0 until totalLines) {
                val line1 = i + 1
                if (lineOwners[i] == null && line1 in minLine..maxLine) {
                    lineOwners[i] = Pair(currentOid, line1)
                }
            }
            break
        }

        if (parentContent == currentContent) {
            currentOid = parentOid
            continue
        }

        // Diff parent vs current
        val edits = diffLines(parentContent, currentContent)

        for (edit in edits) {
            if (edit.kind == EditKind.INSERT && edit.newLine > 0) {
                val lineIdx = edit.newLine - 1
                if (lineIdx < totalLines) {
                    val line1 = lineIdx + 1
                    if (lineOwners[lineIdx] == null && line1 in minLine..maxLine) {
                        lineOwners[lineIdx] = Pair(currentOid, line1)
                        remaining--
                    }
                }
            }
        }

        currentOid = parentOid
        currentContent = parentContent
    }

    // Attribute unowned lines to start commit
    for (i in 0 until totalLines) {
        val line1 = i + 1
        if (lineOwners[i] == null && line1 in minLine..maxLine) {
            lineOwners[i] = Pair(startOid, line1)
        }
    }

    // Build hunks from consecutive lines with same commit
    val hunks = mutableListOf<BlameHunk>()
    var i = minLine - 1

    while (i < maxLine) {
        val (commitId, origLine) = lineOwners[i] ?: Pair(startOid, i + 1)
        val startLine = i + 1
        var count = 1

        while (i + count < maxLine) {
            val next = lineOwners[i + count]
            if (next != null && next.first == commitId) {
                count++
            } else {
                break
            }
        }

        // Load author signature
        val sig = try {
            readCommitObj(gitDir, commitId).author
        } catch (_: Exception) {
            null
        }

        val isBoundary = opts.oldestCommit?.let { it == commitId } ?: false

        hunks.add(BlameHunk(
            linesInHunk = count,
            finalCommitId = commitId,
            finalStartLineNumber = startLine,
            finalSignature = sig,
            origCommitId = commitId,
            origStartLineNumber = origLine,
            origPath = null,
            boundary = isBoundary
        ))

        i += count
    }

    return BlameResult(path = path, hunks = hunks, lineCount = totalLines)
}

// --- Internal helpers ---

private fun readCommitObj(gitDir: File, oid: OID): Commit {
    val (objType, data) = readLooseObject(gitDir, oid)
    if (objType != ObjectType.COMMIT) {
        throw MuonGitException.InvalidObject("expected commit, got $objType")
    }
    return parseCommit(oid, data)
}

private fun readBlobAtCommit(gitDir: File, commitOid: OID, path: String): String {
    val commit = readCommitObj(gitDir, commitOid)
    val (treeType, treeData) = readLooseObject(gitDir, commit.treeId)
    if (treeType != ObjectType.TREE) {
        throw MuonGitException.InvalidObject("expected tree")
    }
    val tree = parseTree(commit.treeId, treeData)

    val entry = findTreeEntryByPath(gitDir, tree.entries, path)

    val (blobType, blobData) = readLooseObject(gitDir, entry.oid)
    if (blobType != ObjectType.BLOB) {
        throw MuonGitException.InvalidObject("expected blob")
    }
    return blobData.decodeToString()
}

private fun findTreeEntryByPath(gitDir: File, entries: List<TreeEntry>, path: String): TreeEntry {
    val slashIdx = path.indexOf('/')
    val name: String
    val rest: String?

    if (slashIdx < 0) {
        name = path
        rest = null
    } else {
        name = path.substring(0, slashIdx)
        rest = path.substring(slashIdx + 1)
    }

    val entry = entries.firstOrNull { it.name == name }
        ?: throw MuonGitException.NotFound("path not found: $path")

    if (rest == null) {
        return entry
    }

    // Subdirectory — recurse
    val (subType, subData) = readLooseObject(gitDir, entry.oid)
    if (subType != ObjectType.TREE) {
        throw MuonGitException.InvalidObject("expected tree for directory $name")
    }
    val subTree = parseTree(entry.oid, subData)
    return findTreeEntryByPath(gitDir, subTree.entries, rest)
}
