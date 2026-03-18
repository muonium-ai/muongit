package ai.muonium.muongit

import java.io.File

/// Git reference reading and resolution.
/// Parity: libgit2 src/libgit2/refs.c

/**
 * Read a reference value (raw, without following symbolic refs).
 *
 * Checks the loose ref file first, then falls back to packed-refs.
 *
 * @param gitDir Path to the .git directory
 * @param name Reference name (e.g. "refs/heads/main", "HEAD")
 * @return The raw reference content (either "ref: <target>" for symbolic, or a hex OID)
 * @throws MuonGitException.NotFound if the reference does not exist
 */
fun readReference(gitDir: File, name: String): String {
    // Check loose ref first
    val looseFile = File(gitDir, name)
    if (looseFile.exists() && looseFile.isFile) {
        return looseFile.readText().trim()
    }

    // Fall back to packed-refs
    val packedRefsFile = File(gitDir, "packed-refs")
    if (packedRefsFile.exists()) {
        for (line in packedRefsFile.readLines()) {
            // Skip comments and peel lines
            if (line.startsWith('#') || line.startsWith('^')) continue
            val trimmed = line.trim()
            if (trimmed.isEmpty()) continue

            // Format: "<hex-oid> <refname>"
            val spaceIndex = trimmed.indexOf(' ')
            if (spaceIndex < 0) continue

            val refName = trimmed.substring(spaceIndex + 1)
            if (refName == name) {
                return trimmed.substring(0, spaceIndex)
            }
        }
    }

    throw MuonGitException.NotFound("reference not found: $name")
}

/**
 * Resolve a reference to a final OID by following symbolic refs.
 *
 * @param gitDir Path to the .git directory
 * @param name Reference name (e.g. "HEAD", "refs/heads/main")
 * @return The resolved OID
 * @throws MuonGitException.NotFound if the reference chain cannot be resolved
 */
fun resolveReference(gitDir: File, name: String): OID {
    var current = name
    val maxDepth = 10 // prevent infinite loops on circular refs

    for (i in 0 until maxDepth) {
        val value = readReference(gitDir, current)
        if (value.startsWith("ref: ")) {
            current = value.removePrefix("ref: ").trim()
        } else {
            // Should be a hex OID
            return OID(value)
        }
    }

    throw MuonGitException.NotFound("reference resolution exceeded max depth: $name")
}

/**
 * List all references in the repository.
 *
 * Returns both loose refs and packed refs (loose takes precedence for duplicates).
 *
 * @param gitDir Path to the .git directory
 * @return List of (refname, value) pairs
 */
fun listReferences(gitDir: File): List<Pair<String, String>> {
    val refs = mutableMapOf<String, String>()

    // Read packed-refs first (loose refs override these)
    val packedRefsFile = File(gitDir, "packed-refs")
    if (packedRefsFile.exists()) {
        for (line in packedRefsFile.readLines()) {
            if (line.startsWith('#') || line.startsWith('^')) continue
            val trimmed = line.trim()
            if (trimmed.isEmpty()) continue

            val spaceIndex = trimmed.indexOf(' ')
            if (spaceIndex < 0) continue

            val oid = trimmed.substring(0, spaceIndex)
            val refName = trimmed.substring(spaceIndex + 1)
            refs[refName] = oid
        }
    }

    // Walk loose refs under refs/
    val refsDir = File(gitDir, "refs")
    if (refsDir.isDirectory) {
        collectLooseRefs(refsDir, "refs", refs)
    }

    return refs.map { (name, value) -> Pair(name, value) }
        .sortedBy { it.first }
}

/**
 * Recursively collect loose refs from a directory.
 */
private fun collectLooseRefs(dir: File, prefix: String, result: MutableMap<String, String>) {
    val entries = dir.listFiles() ?: return
    for (entry in entries.sortedBy { it.name }) {
        val refName = "$prefix/${entry.name}"
        if (entry.isDirectory) {
            collectLooseRefs(entry, refName, result)
        } else if (entry.isFile) {
            val content = entry.readText().trim()
            if (content.isNotEmpty()) {
                result[refName] = content
            }
        }
    }
}
