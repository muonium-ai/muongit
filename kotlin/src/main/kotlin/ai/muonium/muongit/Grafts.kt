package ai.muonium.muongit

import java.io.File

/** A graft entry: a commit with overridden parents */
data class Graft(
    val oid: OID,
    val parents: List<OID>
)

/** A collection of grafts loaded from .git/info/grafts or .git/shallow */
class Grafts {
    private val entries = mutableMapOf<String, Graft>()

    /** Number of graft entries */
    val count: Int get() = entries.size

    /** Parse grafts from content string. Format: COMMIT_OID [PARENT_OID ...] */
    fun parse(content: String) {
        for (line in content.lines()) {
            val trimmed = line.trim()
            if (trimmed.isEmpty() || trimmed.startsWith('#')) continue
            val parts = trimmed.split("\\s+".toRegex())
            if (parts.isEmpty()) continue
            val oid = OID(parts[0])
            val parents = parts.drop(1).map { OID(it) }
            add(Graft(oid = oid, parents = parents))
        }
    }

    /** Add a graft entry */
    fun add(graft: Graft) {
        entries[graft.oid.hex] = graft
    }

    /** Remove a graft entry */
    fun remove(oid: OID): Boolean = entries.remove(oid.hex) != null

    /** Look up a graft for a commit */
    fun get(oid: OID): Graft? = entries[oid.hex]

    /** Check if a commit has a graft */
    fun contains(oid: OID): Boolean = entries.containsKey(oid.hex)

    /** Get parents for a commit */
    fun getParents(oid: OID): List<OID>? = entries[oid.hex]?.parents

    companion object {
        /** Load grafts for a repository (checks info/grafts) */
        fun loadForRepo(gitDir: File): Grafts {
            val graftsPath = File(gitDir, "info/grafts")
            val grafts = Grafts()
            if (graftsPath.exists()) {
                grafts.parse(graftsPath.readText())
            }
            return grafts
        }

        /** Load shallow entries */
        fun loadShallow(gitDir: File): Grafts {
            val shallowPath = File(gitDir, "shallow")
            val grafts = Grafts()
            if (shallowPath.exists()) {
                for (line in shallowPath.readLines()) {
                    val trimmed = line.trim()
                    if (trimmed.isEmpty() || trimmed.startsWith('#')) continue
                    grafts.add(Graft(oid = OID(trimmed), parents = emptyList()))
                }
            }
            return grafts
        }
    }
}
