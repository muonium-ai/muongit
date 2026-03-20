package ai.muonium.muongit

import java.io.File

class Revwalk(private val gitDir: File) {
    private val roots = mutableListOf<OID>()
    private val hidden = mutableListOf<OID>()
    private var sortMode: Int = SORT_NONE
    private var firstParentOnly = false
    private var prepared: List<OID>? = null
    private var cursor = 0

    fun reset() {
        roots.clear()
        hidden.clear()
        firstParentOnly = false
        invalidate()
    }

    fun sorting(sortMode: Int) {
        this.sortMode = sortMode
        invalidate()
    }

    fun simplifyFirstParent() {
        firstParentOnly = true
        invalidate()
    }

    fun push(oid: OID) {
        roots += oid
        invalidate()
    }

    fun pushHead() {
        push(resolveReference(gitDir, "HEAD"))
    }

    fun pushRef(refName: String) {
        push(resolveReference(gitDir, refName))
    }

    fun hide(oid: OID) {
        hidden += oid
        invalidate()
    }

    fun hideHead() {
        hide(resolveReference(gitDir, "HEAD"))
    }

    fun hideRef(refName: String) {
        hide(resolveReference(gitDir, refName))
    }

    fun push(revSpec: RevSpec) {
        if (!revSpec.isRange) {
            val oid = revSpec.to ?: throw MuonGitException.InvalidSpec("revspec is missing a target commit")
            push(oid)
            return
        }

        val from = revSpec.from ?: throw MuonGitException.InvalidSpec("range is missing a left-hand side")
        val to = revSpec.to ?: throw MuonGitException.InvalidSpec("range is missing a right-hand side")

        if (revSpec.usesMergeBase) {
            push(from)
            push(to)
            for (base in revwalkMergeBases(gitDir, from, to)) {
                hide(base)
            }
        } else {
            push(to)
            hide(from)
        }
    }

    fun pushRange(spec: String) {
        val revSpec = revparse(gitDir, spec)
        if (!revSpec.isRange) {
            throw MuonGitException.InvalidSpec("'$spec' is not a revision range")
        }
        push(revSpec)
    }

    fun next(): OID? {
        prepare()
        val commits = prepared ?: return null
        if (cursor >= commits.size) return null
        return commits[cursor++]
    }

    fun allOids(): List<OID> {
        prepare()
        return prepared ?: emptyList()
    }

    private fun invalidate() {
        prepared = null
        cursor = 0
    }

    private fun prepare() {
        if (prepared != null) return

        val hiddenSet = collectRevisionAncestors(gitDir, hidden, firstParentOnly)
        val commits = collectVisibleRevisionCommits(gitDir, roots, hiddenSet, firstParentOnly)

        var ordered = if ((sortMode and SORT_TOPOLOGICAL) != 0) {
            topoSortRevisionCommits(commits, sortMode, firstParentOnly)
        } else {
            commits.keys.sortedWith { left, right ->
                compareRevisionCommits(left, right, commits, sortMode)
            }
        }

        if ((sortMode and SORT_REVERSE) != 0) {
            ordered = ordered.reversed()
        }

        prepared = ordered
        cursor = 0
    }

    companion object {
        const val SORT_NONE = 0
        const val SORT_TOPOLOGICAL = 1 shl 0
        const val SORT_TIME = 1 shl 1
        const val SORT_REVERSE = 1 shl 2
    }
}

private fun collectRevisionAncestors(
    gitDir: File,
    starts: List<OID>,
    firstParentOnly: Boolean,
): Set<OID> {
    val visited = linkedSetOf<OID>()
    val queue = ArrayDeque<OID>()
    for (oid in starts) {
        if (visited.add(oid)) {
            queue.addLast(oid)
        }
    }

    while (queue.isNotEmpty()) {
        val oid = queue.removeFirst()
        val commit = revisionReadCommit(gitDir, oid)
        for (parent in revisionSelectedParents(commit, firstParentOnly)) {
            if (visited.add(parent)) {
                queue.addLast(parent)
            }
        }
    }

    return visited
}

private fun collectVisibleRevisionCommits(
    gitDir: File,
    roots: List<OID>,
    hidden: Set<OID>,
    firstParentOnly: Boolean,
): Map<OID, Commit> {
    val commits = linkedMapOf<OID, Commit>()
    val queue = ArrayDeque<OID>()
    val seen = mutableSetOf<OID>()

    for (oid in roots) {
        if (!hidden.contains(oid) && seen.add(oid)) {
            queue.addLast(oid)
        }
    }

    while (queue.isNotEmpty()) {
        val oid = queue.removeFirst()
        if (hidden.contains(oid)) {
            continue
        }

        val commit = revisionReadCommit(gitDir, oid)
        for (parent in revisionSelectedParents(commit, firstParentOnly)) {
            if (!hidden.contains(parent) && seen.add(parent)) {
                queue.addLast(parent)
            }
        }
        commits[oid] = commit
    }

    return commits
}

private fun topoSortRevisionCommits(
    commits: Map<OID, Commit>,
    sortMode: Int,
    firstParentOnly: Boolean,
): List<OID> {
    val childCounts = commits.keys.associateWithTo(linkedMapOf()) { 0 }

    for (commit in commits.values) {
        for (parent in revisionSelectedParents(commit, firstParentOnly)) {
            if (childCounts.containsKey(parent)) {
                childCounts[parent] = childCounts.getValue(parent) + 1
            }
        }
    }

    val ready = childCounts
        .filterValues { it == 0 }
        .keys
        .sortedWith { left, right -> compareRevisionCommits(left, right, commits, sortMode) }
        .toMutableList()

    val ordered = mutableListOf<OID>()
    while (ready.isNotEmpty()) {
        val oid = ready.removeAt(0)
        ordered += oid

        val commit = commits[oid] ?: continue
        for (parent in revisionSelectedParents(commit, firstParentOnly)) {
            if (!childCounts.containsKey(parent)) continue
            childCounts[parent] = childCounts.getValue(parent) - 1
            if (childCounts.getValue(parent) == 0) {
                ready += parent
            }
        }
        ready.sortWith { left, right -> compareRevisionCommits(left, right, commits, sortMode) }
    }

    return ordered
}

private fun compareRevisionCommits(
    left: OID,
    right: OID,
    commits: Map<OID, Commit>,
    sortMode: Int,
): Int {
    val usesTime = sortMode == Revwalk.SORT_NONE || (sortMode and Revwalk.SORT_TIME) != 0
    if (usesTime) {
        val leftTime = commits[left]?.committer?.time ?: 0L
        val rightTime = commits[right]?.committer?.time ?: 0L
        if (leftTime != rightTime) {
            return when {
                leftTime > rightTime -> -1
                else -> 1
            }
        }
    }
    return left.hex.compareTo(right.hex)
}

private fun revisionSelectedParents(commit: Commit, firstParentOnly: Boolean): List<OID> =
    if (firstParentOnly && commit.parentIds.isNotEmpty()) listOf(commit.parentIds.first()) else commit.parentIds

private fun revwalkMergeBases(gitDir: File, left: OID, right: OID): List<OID> {
    if (left == right) return listOf(left)

    val leftAncestors = collectRevisionAncestors(gitDir, listOf(left), false)
    val common = mutableListOf<OID>()
    val visited = mutableSetOf(right)
    val queue = ArrayDeque<OID>()
    queue.addLast(right)

    while (queue.isNotEmpty()) {
        val oid = queue.removeFirst()
        if (leftAncestors.contains(oid)) {
            common += oid
            continue
        }

        val commit = revisionReadCommit(gitDir, oid)
        for (parent in commit.parentIds) {
            if (visited.add(parent)) {
                queue.addLast(parent)
            }
        }
    }

    var best = common.toList()
    for (candidate in common) {
        val candidateAncestors = collectRevisionAncestors(gitDir, listOf(candidate), false)
        best = best.filter { it == candidate || !candidateAncestors.contains(it) }
    }

    return best.distinct().sortedBy { it.hex }
}
