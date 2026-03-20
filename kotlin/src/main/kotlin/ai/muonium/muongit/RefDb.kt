package ai.muonium.muongit

import java.io.File

data class Reference(
    val name: String,
    val value: String,
    val symbolicTarget: String?,
    val target: OID?,
) {
    val isSymbolic: Boolean get() = symbolicTarget != null
}

class RefDb(val gitDir: File) {
    fun read(name: String): Reference {
        val value = readReference(gitDir, name).trim()
        return if (value.startsWith("ref: ")) {
            Reference(
                name = name,
                value = value,
                symbolicTarget = value.removePrefix("ref: ").trim(),
                target = null,
            )
        } else {
            Reference(name = name, value = value, symbolicTarget = null, target = OID(value))
        }
    }

    fun resolve(name: String): OID = resolveReference(gitDir, name)

    fun list(): List<Reference> =
        listReferences(gitDir).map { (name, value) -> Reference(name, value.trim(), value.trim().removePrefix("ref: ").takeIf { value.trim().startsWith("ref: ") }?.trim(), if (value.trim().startsWith("ref: ")) null else OID(value.trim())) }

    fun write(name: String, oid: OID) {
        writeReference(gitDir, name, oid)
    }

    fun writeSymbolic(name: String, target: String) {
        writeSymbolicReference(gitDir, name, target)
    }

    fun delete(name: String): Boolean {
        val looseDeleted = deleteReference(gitDir, name)
        val packedDeleted = deletePackedReference(gitDir, name)
        return looseDeleted || packedDeleted
    }
}

fun Repository.refdb(): RefDb = RefDb(gitDir)

internal fun packedReferences(gitDir: File): MutableMap<String, String> {
    val packedFile = File(gitDir, "packed-refs")
    if (!packedFile.exists()) {
        return linkedMapOf()
    }

    val refs = linkedMapOf<String, String>()
    for (line in packedFile.readLines()) {
        val trimmed = line.trim()
        if (trimmed.isEmpty() || trimmed.startsWith('#') || trimmed.startsWith('^')) continue
        val spaceIndex = trimmed.indexOf(' ')
        if (spaceIndex < 0) continue
        refs[trimmed.substring(spaceIndex + 1)] = trimmed.substring(0, spaceIndex)
    }
    return refs
}

private fun deletePackedReference(gitDir: File, name: String): Boolean {
    val refs = packedReferences(gitDir)
    val deleted = refs.remove(name) != null
    if (deleted) {
        writePackedReferences(gitDir, refs)
    }
    return deleted
}

private fun writePackedReferences(gitDir: File, refs: Map<String, String>) {
    val packedFile = File(gitDir, "packed-refs")
    if (refs.isEmpty()) {
        if (packedFile.exists()) {
            packedFile.delete()
        }
        return
    }

    val lines = mutableListOf("# pack-refs with: sorted")
    for (name in refs.keys.sorted()) {
        lines += "${refs.getValue(name)} $name"
    }
    packedFile.writeText(lines.joinToString(separator = "\n", postfix = "\n"))
}
