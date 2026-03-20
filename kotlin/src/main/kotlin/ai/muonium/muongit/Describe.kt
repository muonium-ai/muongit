package ai.muonium.muongit

import java.io.File
import java.util.LinkedList

/** Strategy for finding tags in describe */
enum class DescribeStrategy {
    /** Only annotated tags (default) */
    DEFAULT,
    /** All tags (annotated + lightweight) */
    TAGS,
    /** All refs */
    ALL
}

/** Options for describe */
data class DescribeOptions(
    val strategy: DescribeStrategy = DescribeStrategy.DEFAULT,
    val maxCandidates: Int = 10,
    val pattern: String? = null,
    val onlyFollowFirstParent: Boolean = false,
    val showCommitOidAsFallback: Boolean = false
)

/** Options for formatting a describe result */
data class DescribeFormatOptions(
    val abbreviatedSize: Int = 7,
    val alwaysUseLongFormat: Boolean = false,
    val dirtySuffix: String? = null
)

/** Result of a describe operation */
data class DescribeResult(
    val tagName: String?,
    val depth: Int,
    val commitId: OID,
    val exactMatch: Boolean,
    val fallbackToId: Boolean
) {
    /** Format the describe result as a string */
    fun format(opts: DescribeFormatOptions = DescribeFormatOptions()): String {
        val base = when {
            fallbackToId -> commitId.hex.take(opts.abbreviatedSize)
            tagName != null -> {
                if (exactMatch && !opts.alwaysUseLongFormat) {
                    tagName
                } else {
                    val abbrev = commitId.hex.take(opts.abbreviatedSize)
                    "$tagName-$depth-g$abbrev"
                }
            }
            else -> commitId.hex.take(opts.abbreviatedSize)
        }
        return if (opts.dirtySuffix != null) base + opts.dirtySuffix else base
    }
}

private data class TagCandidate(
    val name: String,
    val priority: Int,
    val commitOid: OID
)

/**
 * Describe a commit — find the most recent tag reachable from it.
 * Walks commit history via BFS to find the nearest tag ancestor.
 */
fun describe(gitDir: File, commitOid: OID, opts: DescribeOptions = DescribeOptions()): DescribeResult {
    val candidates = collectCandidates(gitDir, opts)

    // Check if commit itself is tagged
    candidates[commitOid.hex]?.let { candidate ->
        return DescribeResult(
            tagName = candidate.name,
            depth = 0,
            commitId = commitOid,
            exactMatch = true,
            fallbackToId = false
        )
    }

    // BFS from commit through parents
    val visited = mutableSetOf(commitOid.hex)
    val queue = LinkedList<Pair<OID, Int>>()
    queue.add(commitOid to 0)
    var best: Pair<TagCandidate, Int>? = null

    while (queue.isNotEmpty()) {
        val (oid, depth) = queue.poll()

        candidates[oid.hex]?.let { candidate ->
            val dominated = best?.let { (currentBest, currentDepth) ->
                depth < currentDepth || (depth == currentDepth && candidate.priority > currentBest.priority)
            } ?: true

            if (dominated) best = candidate to depth
            if (best != null && depth > best!!.second + opts.maxCandidates) return@let
            return@let // don't continue past found tags
        }

        // Read commit and enqueue parents
        try {
            val (objType, data) = readLooseObject(gitDir, oid)
            if (objType == ObjectType.COMMIT) {
                val commit = parseCommit(oid, data)
                val parents = if (opts.onlyFollowFirstParent) {
                    commit.parentIds.take(1)
                } else {
                    commit.parentIds
                }
                for (parentOid in parents) {
                    if (visited.add(parentOid.hex)) {
                        queue.add(parentOid to depth + 1)
                    }
                }
            }
        } catch (_: Exception) { /* skip unreadable objects */ }
    }

    return best?.let { (candidate, depth) ->
        DescribeResult(
            tagName = candidate.name,
            depth = depth,
            commitId = commitOid,
            exactMatch = false,
            fallbackToId = false
        )
    } ?: if (opts.showCommitOidAsFallback) {
        DescribeResult(tagName = null, depth = 0, commitId = commitOid, exactMatch = false, fallbackToId = true)
    } else {
        throw MuonGitException.NotFound("no tag found for describe")
    }
}

private fun collectCandidates(gitDir: File, opts: DescribeOptions): Map<String, TagCandidate> {
    val refs = listReferences(gitDir)
    val candidates = mutableMapOf<String, TagCandidate>()

    for ((refname, value) in refs) {
        val (name, priority) = categorizeRef(refname, opts) ?: continue

        if (opts.pattern != null && !globMatch(opts.pattern, name)) continue

        val oid = try { OID(value) } catch (_: Exception) { continue }
        val (commitOid, actualPriority) = peelToCommit(gitDir, oid, priority)

        candidates[commitOid.hex] = TagCandidate(name, actualPriority, commitOid)
    }

    return candidates
}

private fun categorizeRef(refname: String, opts: DescribeOptions): Pair<String, Int>? {
    return when (opts.strategy) {
        DescribeStrategy.DEFAULT -> {
            if (refname.startsWith("refs/tags/")) refname.removePrefix("refs/tags/") to 2
            else null
        }
        DescribeStrategy.TAGS -> {
            if (refname.startsWith("refs/tags/")) refname.removePrefix("refs/tags/") to 1
            else null
        }
        DescribeStrategy.ALL -> {
            when {
                refname.startsWith("refs/tags/") -> refname.removePrefix("refs/tags/") to 2
                refname.startsWith("refs/heads/") -> "heads/${refname.removePrefix("refs/heads/")}" to 0
                refname.startsWith("refs/remotes/") -> "remotes/${refname.removePrefix("refs/remotes/")}" to 0
                else -> refname to 0
            }
        }
    }
}

private fun peelToCommit(gitDir: File, oid: OID, defaultPriority: Int): Pair<OID, Int> {
    try {
        val (objType, data) = readLooseObject(gitDir, oid)
        when (objType) {
            ObjectType.TAG -> {
                val tag = parseTag(oid, data)
                return tag.targetId to 2
            }
            ObjectType.COMMIT -> return oid to defaultPriority
            else -> {}
        }
    } catch (_: Exception) { /* ignore */ }
    return oid to defaultPriority
}
