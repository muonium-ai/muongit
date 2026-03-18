package ai.muonium.muongit

import java.io.ByteArrayOutputStream

/** A parsed git commit object */
data class Commit(
    val oid: OID,
    val treeId: OID,
    val parentIds: List<OID>,
    val author: Signature,
    val committer: Signature,
    val message: String,
    val messageEncoding: String? = null
)

/** Parse a commit object from its raw data content */
fun parseCommit(oid: OID, data: ByteArray): Commit {
    val text = data.decodeToString()

    var treeId: OID? = null
    val parentIds = mutableListOf<OID>()
    var author: Signature? = null
    var committer: Signature? = null
    var messageEncoding: String? = null

    // Split at first blank line
    val blankIdx = text.indexOf("\n\n")
    val headerSection = if (blankIdx >= 0) text.substring(0, blankIdx) else text
    val message = if (blankIdx >= 0) text.substring(blankIdx + 2) else ""

    for (line in headerSection.split("\n")) {
        when {
            line.startsWith("tree ") -> treeId = OID(line.removePrefix("tree "))
            line.startsWith("parent ") -> parentIds.add(OID(line.removePrefix("parent ")))
            line.startsWith("author ") -> author = parseSignatureLine(line.removePrefix("author "))
            line.startsWith("committer ") -> committer = parseSignatureLine(line.removePrefix("committer "))
            line.startsWith("encoding ") -> messageEncoding = line.removePrefix("encoding ")
        }
    }

    return Commit(
        oid = oid,
        treeId = treeId ?: throw MuonGitException.InvalidObject("commit missing tree"),
        parentIds = parentIds,
        author = author ?: throw MuonGitException.InvalidObject("commit missing author"),
        committer = committer ?: throw MuonGitException.InvalidObject("commit missing committer"),
        message = message,
        messageEncoding = messageEncoding
    )
}

private val HEX_CHARS = "0123456789abcdef".toByteArray()
private val TREE_PREFIX = "tree ".toByteArray()
private val PARENT_PREFIX = "parent ".toByteArray()
private val AUTHOR_PREFIX = "author ".toByteArray()
private val COMMITTER_PREFIX = "committer ".toByteArray()
private val ENCODING_PREFIX = "encoding ".toByteArray()
private val SPACE_LT = " <".toByteArray()
private val GT_SPACE = "> ".toByteArray()

/** Serialize a commit to its raw data representation (without the object header) */
fun serializeCommit(
    treeId: OID,
    parentIds: List<OID>,
    author: Signature,
    committer: Signature,
    message: String,
    messageEncoding: String? = null
): ByteArray {
    val buf = ByteArrayOutputStream(256 + message.length)
    buf.write(TREE_PREFIX)
    writeHex(treeId.raw, buf)
    buf.write('\n'.code)
    for (pid in parentIds) {
        buf.write(PARENT_PREFIX)
        writeHex(pid.raw, buf)
        buf.write('\n'.code)
    }
    buf.write(AUTHOR_PREFIX)
    writeSignatureBytes(author, buf)
    buf.write('\n'.code)
    buf.write(COMMITTER_PREFIX)
    writeSignatureBytes(committer, buf)
    buf.write('\n'.code)
    if (messageEncoding != null) {
        buf.write(ENCODING_PREFIX)
        buf.write(messageEncoding.toByteArray())
        buf.write('\n'.code)
    }
    buf.write('\n'.code)
    buf.write(message.toByteArray())
    return buf.toByteArray()
}

private fun writeHex(raw: ByteArray, buf: ByteArrayOutputStream) {
    for (b in raw) {
        buf.write(HEX_CHARS[(b.toInt() shr 4) and 0x0F].toInt())
        buf.write(HEX_CHARS[b.toInt() and 0x0F].toInt())
    }
}

private fun writeSignatureBytes(sig: Signature, buf: ByteArrayOutputStream) {
    buf.write(sig.name.toByteArray())
    buf.write(SPACE_LT)
    buf.write(sig.email.toByteArray())
    buf.write(GT_SPACE)
    buf.write(sig.time.toString().toByteArray())
    buf.write(' '.code)
    buf.write(if (sig.offset >= 0) '+'.code else '-'.code)
    val abs = kotlin.math.abs(sig.offset)
    val hours = abs / 60
    val minutes = abs % 60
    buf.write('0'.code + hours / 10)
    buf.write('0'.code + hours % 10)
    buf.write('0'.code + minutes / 10)
    buf.write('0'.code + minutes % 10)
}

/** Parse "Name <email> timestamp offset" into a Signature */
internal fun parseSignatureLine(s: String): Signature {
    val emailStart = s.indexOf('<')
    val emailEnd = s.indexOf('>')
    if (emailStart < 0 || emailEnd < 0) return Signature(name = s, email = "")

    val name = s.substring(0, emailStart).trim()
    val email = s.substring(emailStart + 1, emailEnd)
    val remainder = s.substring(emailEnd + 1).trim()
    val parts = remainder.split(" ")

    val time = parts.getOrNull(0)?.toLongOrNull() ?: 0L
    val offset = parts.getOrNull(1)?.let { parseTimezoneOffset(it) } ?: 0

    return Signature(name = name, email = email, time = time, offset = offset)
}

/** Format a Signature into "Name <email> timestamp offset" */
fun formatSignatureLine(sig: Signature): String {
    val sign = if (sig.offset >= 0) "+" else "-"
    val abs = kotlin.math.abs(sig.offset)
    val hours = abs / 60
    val minutes = abs % 60
    return "${sig.name} <${sig.email}> ${sig.time} $sign${"%02d%02d".format(hours, minutes)}"
}

/** Parse "+0530" or "-0800" into minutes offset */
internal fun parseTimezoneOffset(s: String): Int {
    if (s.length < 5) return 0
    val sign = if (s.startsWith("-")) -1 else 1
    val digits = s.drop(1)
    if (digits.length != 4) return 0
    val hours = digits.substring(0, 2).toIntOrNull() ?: return 0
    val minutes = digits.substring(2, 4).toIntOrNull() ?: return 0
    return sign * (hours * 60 + minutes)
}
