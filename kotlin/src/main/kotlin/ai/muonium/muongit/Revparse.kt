package ai.muonium.muongit

import java.io.File

/** A parsed revision expression. */
data class RevSpec(
    val from: OID?,
    val to: OID?,
    val isRange: Boolean,
    val usesMergeBase: Boolean,
)

/**
 * Resolve a common revision expression to a commit OID.
 *
 * Supported subset:
 * - full OIDs
 * - refs and short refs like `main`, `tags/v1`, `origin/main`
 * - `HEAD^`, `HEAD^N`, `HEAD~N`
 */
fun resolveRevision(gitDir: File, spec: String): OID {
    val trimmed = spec.trim()
    if (trimmed.isEmpty()) {
        throw MuonGitException.InvalidSpec("empty revision spec")
    }
    if (trimmed.contains("...") || isTwoDotRange(trimmed)) {
        throw MuonGitException.InvalidSpec("range '$trimmed' does not resolve to a single revision")
    }

    val (baseSpec, suffix) = splitBaseAndSuffix(trimmed)
    var current = readObject(gitDir, resolveRevisionBase(gitDir, baseSpec))

    var index = 0
    while (index < suffix.length) {
        when (suffix[index]) {
            '~' -> {
                index += 1
                val start = index
                while (index < suffix.length && suffix[index].isDigit()) {
                    index += 1
                }
                val count = if (start == index) 1 else suffix.substring(start, index).toIntOrNull()
                    ?: throw MuonGitException.InvalidSpec("invalid ancestry operator in '$trimmed'")
                repeat(count) {
                    val commit = peelRevisionCommit(gitDir, current, trimmed)
                    val parent = commit.parentIds.firstOrNull()
                        ?: throw MuonGitException.InvalidSpec("revision '$trimmed' has no first parent")
                    current = readObject(gitDir, parent)
                }
            }
            '^' -> {
                index += 1
                val start = index
                while (index < suffix.length && suffix[index].isDigit()) {
                    index += 1
                }
                val parentIndex = if (start == index) 1 else suffix.substring(start, index).toIntOrNull()
                    ?: throw MuonGitException.InvalidSpec("invalid parent selector in '$trimmed'")
                val commit = peelRevisionCommit(gitDir, current, trimmed)
                if (parentIndex == 0) {
                    current = readObject(gitDir, commit.oid)
                    continue
                }
                val parent = commit.parentIds.getOrNull(parentIndex - 1)
                    ?: throw MuonGitException.InvalidSpec("revision '$trimmed' has no parent $parentIndex")
                current = readObject(gitDir, parent)
            }
            else -> throw MuonGitException.InvalidSpec("unsupported revision syntax '$trimmed'")
        }
    }

    return peelRevisionCommit(gitDir, current, trimmed).oid
}

fun revparseSingle(gitDir: File, spec: String): GitObject =
    readObject(gitDir, resolveRevision(gitDir, spec))

fun revparse(gitDir: File, spec: String): RevSpec {
    val trimmed = spec.trim()
    if (trimmed.isEmpty()) {
        throw MuonGitException.InvalidSpec("empty revision spec")
    }

    splitRange(trimmed, "...")?.let { (left, right) ->
        return RevSpec(
            from = resolveRevision(gitDir, left),
            to = resolveRevision(gitDir, right),
            isRange = true,
            usesMergeBase = true,
        )
    }

    splitTwoDotRange(trimmed)?.let { (left, right) ->
        return RevSpec(
            from = resolveRevision(gitDir, left),
            to = resolveRevision(gitDir, right),
            isRange = true,
            usesMergeBase = false,
        )
    }

    return RevSpec(
        from = null,
        to = resolveRevision(gitDir, trimmed),
        isRange = false,
        usesMergeBase = false,
    )
}

internal fun revisionReadCommit(gitDir: File, oid: OID): Commit {
    val objectData = readObject(gitDir, oid)
    if (objectData.objectType != ObjectType.COMMIT) {
        throw MuonGitException.InvalidSpec("revision '${oid.hex}' is not a commit")
    }
    return objectData.asCommit()
}

private fun resolveRevisionBase(gitDir: File, spec: String): OID {
    val trimmed = spec.trim()
    if (trimmed.isEmpty()) {
        throw MuonGitException.InvalidSpec("missing base revision")
    }

    if (looksLikeFullOID(trimmed)) {
        val oid = OID(trimmed)
        if (runCatching { readObject(gitDir, oid) }.isSuccess) {
            return oid
        }
    }

    for (candidate in revisionReferenceCandidates(trimmed)) {
        val oid = runCatching { resolveReference(gitDir, candidate) }.getOrNull()
        if (oid != null) {
            return oid
        }
    }

    throw MuonGitException.NotFound("could not resolve revision '$trimmed'")
}

private fun peelRevisionCommit(gitDir: File, objectData: GitObject, spec: String): Commit {
    val peeled = if (objectData.objectType == ObjectType.TAG) objectData.peel(gitDir) else objectData
    if (peeled.objectType != ObjectType.COMMIT) {
        throw MuonGitException.InvalidSpec("revision '$spec' does not resolve to a commit")
    }
    return peeled.asCommit()
}

private fun splitBaseAndSuffix(spec: String): Pair<String, String> {
    val index = spec.indexOfFirst { it == '^' || it == '~' }.let { if (it < 0) spec.length else it }
    val base = spec.substring(0, index)
    if (base.isEmpty()) {
        throw MuonGitException.InvalidSpec("missing base revision in '$spec'")
    }
    return base to spec.substring(index)
}

private fun splitRange(spec: String, operator: String): Pair<String, String>? {
    val index = spec.indexOf(operator)
    if (index < 0) return null
    val left = spec.substring(0, index).trim()
    val right = spec.substring(index + operator.length).trim()
    if (left.isEmpty() || right.isEmpty()) return null
    return left to right
}

private fun splitTwoDotRange(spec: String): Pair<String, String>? {
    if (spec.contains("...")) return null
    return splitRange(spec, "..")
}

private fun isTwoDotRange(spec: String): Boolean = splitTwoDotRange(spec) != null

private fun looksLikeFullOID(spec: String): Boolean =
    spec.length == 40 && spec.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }

private fun revisionReferenceCandidates(spec: String): List<String> {
    val candidates = mutableListOf(spec)
    if (!spec.startsWith("refs/")) {
        candidates += "refs/$spec"
        candidates += "refs/heads/$spec"
        candidates += "refs/tags/$spec"
        candidates += "refs/remotes/$spec"
    }
    return candidates
}
