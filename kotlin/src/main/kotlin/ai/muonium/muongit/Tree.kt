package ai.muonium.muongit

/** File mode constants for tree entries */
object FileMode {
    const val TREE: Int = 0x4000      // 040000 octal
    const val BLOB: Int = 0x81A4      // 100644 octal
    const val BLOB_EXE: Int = 0x81ED  // 100755 octal
    const val LINK: Int = 0xA000      // 120000 octal
    const val GITLINK: Int = 0xE000   // 160000 octal
}

/** A single entry in a tree object */
data class TreeEntry(
    val mode: Int,
    val name: String,
    val oid: OID
) {
    val isTree: Boolean get() = mode == FileMode.TREE
    val isBlob: Boolean get() = mode == FileMode.BLOB || mode == FileMode.BLOB_EXE
}

/** A parsed git tree object */
data class Tree(
    val oid: OID,
    val entries: List<TreeEntry>
)

/** Parse a tree object from its raw binary data */
fun parseTree(oid: OID, data: ByteArray): Tree {
    val entries = mutableListOf<TreeEntry>()
    var i = 0

    while (i < data.size) {
        // Parse mode (octal digits until space)
        val modeStart = i
        while (i < data.size && data[i] != 0x20.toByte()) i++
        if (i >= data.size) throw MuonGitException.InvalidObject("tree entry: missing space after mode")

        val modeStr = String(data, modeStart, i - modeStart, Charsets.UTF_8)
        val mode = modeStr.toIntOrNull(8)
            ?: throw MuonGitException.InvalidObject("tree entry: invalid mode '$modeStr'")
        i++ // skip space

        // Parse name (until null byte)
        val nameStart = i
        while (i < data.size && data[i] != 0.toByte()) i++
        if (i >= data.size) throw MuonGitException.InvalidObject("tree entry: missing null after name")

        val name = String(data, nameStart, i - nameStart, Charsets.UTF_8)
        i++ // skip null

        // Read 20-byte raw OID
        if (i + 20 > data.size) throw MuonGitException.InvalidObject("tree entry: truncated OID")
        val oidBytes = data.copyOfRange(i, i + 20)
        i += 20

        entries.add(TreeEntry(mode = mode, name = name, oid = OID(oidBytes)))
    }

    return Tree(oid = oid, entries = entries)
}

/** Pre-computed octal mode bytes for common git modes */
private fun modeBytes(mode: Int): ByteArray? = when (mode) {
    0x81A4 -> byteArrayOf(0x31,0x30,0x30,0x36,0x34,0x34) // "100644"
    0x4000 -> byteArrayOf(0x34,0x30,0x30,0x30,0x30)       // "40000"
    0x81ED -> byteArrayOf(0x31,0x30,0x30,0x37,0x35,0x35) // "100755"
    0xA000.toInt() -> byteArrayOf(0x31,0x32,0x30,0x30,0x30,0x30) // "120000"
    0xE000.toInt() -> byteArrayOf(0x31,0x36,0x30,0x30,0x30,0x30) // "160000"
    else -> null
}

/** Serialize tree entries to raw binary data (without the object header).
 *  Entries are sorted by name with tree-sorting rules. */
fun serializeTree(entries: List<TreeEntry>): ByteArray {
    val sorted = entries.sortedBy { entry ->
        if (entry.isTree) "${entry.name}/" else entry.name
    }

    // Pre-allocate: each entry ~28 bytes
    val buf = java.io.ByteArrayOutputStream(entries.size * 40)
    for (entry in sorted) {
        val mb = modeBytes(entry.mode)
        if (mb != null) {
            buf.write(mb)
        } else {
            buf.write(entry.mode.toString(8).toByteArray(Charsets.UTF_8))
        }
        buf.write(0x20) // space
        buf.write(entry.name.toByteArray(Charsets.UTF_8))
        buf.write(0x00) // null
        buf.write(entry.oid.raw)
    }
    return buf.toByteArray()
}
