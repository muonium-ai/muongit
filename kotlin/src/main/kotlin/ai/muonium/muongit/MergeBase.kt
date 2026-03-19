package ai.muonium.muongit

import java.io.File
import java.util.LinkedList

/** Read and parse a commit from the object database. */
private fun readCommit(gitDir: File, oid: OID): Commit {
    val (objType, data) = readLooseObject(gitDir, oid)
    require(objType == ObjectType.COMMIT) { "expected commit, got $objType" }
    return parseCommit(oid, data)
}

/** Collect all ancestors of a commit (including itself) via BFS. */
private fun ancestors(gitDir: File, oid: OID): Set<OID> {
    val visited = mutableSetOf(oid)
    val queue = LinkedList<OID>()
    queue.add(oid)

    while (queue.isNotEmpty()) {
        val current = queue.poll()
        val commit = readCommit(gitDir, current)
        for (parentId in commit.parentIds) {
            if (visited.add(parentId)) {
                queue.add(parentId)
            }
        }
    }

    return visited
}

/**
 * Find the merge base (lowest common ancestor) of two commits.
 *
 * Returns the best common ancestor — one that is not an ancestor of any
 * other common ancestor. Returns null if the commits share no history.
 */
fun mergeBase(gitDir: File, oid1: OID, oid2: OID): OID? {
    if (oid1 == oid2) return oid1

    val ancestors1 = ancestors(gitDir, oid1)

    val common = mutableListOf<OID>()
    val visited = mutableSetOf(oid2)
    val queue = LinkedList<OID>()
    queue.add(oid2)

    while (queue.isNotEmpty()) {
        val current = queue.poll()
        if (current in ancestors1) {
            common.add(current)
            continue
        }
        val commit = readCommit(gitDir, current)
        for (parentId in commit.parentIds) {
            if (visited.add(parentId)) {
                queue.add(parentId)
            }
        }
    }

    if (common.isEmpty()) return null
    if (common.size == 1) return common[0]

    // Filter: remove any common ancestor that is an ancestor of another
    var best = common.toList()
    for (ca in common) {
        val caAncestors = ancestors(gitDir, ca)
        best = best.filter { it == ca || it !in caAncestors }
    }

    return best.firstOrNull()
}

/**
 * Find all merge bases between two commits.
 * In simple cases this returns one OID; for criss-cross merges it may return multiple.
 */
fun mergeBases(gitDir: File, oid1: OID, oid2: OID): List<OID> {
    if (oid1 == oid2) return listOf(oid1)

    val ancestors1 = ancestors(gitDir, oid1)

    val common = mutableListOf<OID>()
    val visited = mutableSetOf(oid2)
    val queue = LinkedList<OID>()
    queue.add(oid2)

    while (queue.isNotEmpty()) {
        val current = queue.poll()
        if (current in ancestors1) {
            common.add(current)
            continue
        }
        val commit = readCommit(gitDir, current)
        for (parentId in commit.parentIds) {
            if (visited.add(parentId)) {
                queue.add(parentId)
            }
        }
    }

    var best = common.toList()
    for (ca in common) {
        val caAncestors = ancestors(gitDir, ca)
        best = best.filter { it == ca || it !in caAncestors }
    }

    return best
}
