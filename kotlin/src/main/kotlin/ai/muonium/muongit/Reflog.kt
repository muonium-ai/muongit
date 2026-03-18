package ai.muonium.muongit

import java.io.File

/** A single reflog entry */
data class ReflogEntry(
    val oldOid: OID,
    val newOid: OID,
    val committer: Signature,
    val message: String
)

/** Read the reflog for a given reference name. */
fun readReflog(gitDir: File, refName: String): List<ReflogEntry> {
    val logFile = File(gitDir, "logs/$refName")
    if (!logFile.exists()) return emptyList()
    return parseReflog(logFile.readText())
}

/** Parse reflog file content into entries */
internal fun parseReflog(content: String): List<ReflogEntry> {
    val entries = mutableListOf<ReflogEntry>()

    for (line in content.lines()) {
        val trimmed = line.trim()
        if (trimmed.isEmpty()) continue

        val tabIdx = trimmed.indexOf('\t')
        if (tabIdx < 0) continue

        val sigPart = trimmed.substring(0, tabIdx)
        val message = trimmed.substring(tabIdx + 1)

        val parts = sigPart.split(" ", limit = 3)
        if (parts.size < 3) continue

        val oldOid = OID(parts[0])
        val newOid = OID(parts[1])
        val committer = parseSignatureLine(parts[2])

        entries.add(ReflogEntry(oldOid = oldOid, newOid = newOid, committer = committer, message = message))
    }

    return entries
}

/** Append an entry to the reflog for a given reference. */
fun appendReflog(
    gitDir: File,
    refName: String,
    oldOid: OID,
    newOid: OID,
    committer: Signature,
    message: String
) {
    val logFile = File(gitDir, "logs/$refName")
    logFile.parentFile?.mkdirs()
    val line = formatReflogEntry(oldOid, newOid, committer, message)
    logFile.appendText(line)
}

/** Format a single reflog entry line */
internal fun formatReflogEntry(oldOid: OID, newOid: OID, committer: Signature, message: String): String {
    return "${oldOid.hex} ${newOid.hex} ${formatSignatureLine(committer)}\t$message\n"
}
