// Transport.kt - Git smart protocol and transport abstractions
// Parity: libgit2 src/libgit2/transports/smart_pkt.c

package ai.muonium.muongit

// --- Pkt-line encoding/decoding ---

/** Encode data as a pkt-line with 4-hex-digit length prefix. */
fun pktLineEncode(data: ByteArray): ByteArray {
    val len = data.size + 4
    val header = "%04x".format(len).toByteArray(Charsets.US_ASCII)
    return header + data
}

/** Encode a flush packet (0000). */
fun pktLineFlush(): ByteArray = "0000".toByteArray(Charsets.US_ASCII)

/** Encode a delimiter packet (0001). */
fun pktLineDelim(): ByteArray = "0001".toByteArray(Charsets.US_ASCII)

/** Decoded pkt-line. */
sealed class PktLine {
    data class Data(val bytes: ByteArray) : PktLine() {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Data) return false
            return bytes.contentEquals(other.bytes)
        }
        override fun hashCode(): Int = bytes.contentHashCode()
    }
    data object Flush : PktLine()
    data object Delim : PktLine()
}

/** Parse pkt-lines from a byte buffer. Returns parsed lines and bytes consumed. */
fun pktLineDecode(input: ByteArray): Pair<List<PktLine>, Int> {
    val lines = mutableListOf<PktLine>()
    var pos = 0

    while (pos + 4 <= input.size) {
        val hex = String(input, pos, 4, Charsets.US_ASCII)

        if (hex == "0000") {
            lines.add(PktLine.Flush)
            pos += 4
            continue
        }
        if (hex == "0001") {
            lines.add(PktLine.Delim)
            pos += 4
            continue
        }

        val len = hex.toIntOrNull(16)
            ?: throw MuonGitException.InvalidObject("invalid pkt-line length")

        if (len < 4) {
            throw MuonGitException.InvalidObject("pkt-line length too small")
        }

        if (pos + len > input.size) {
            break // Incomplete packet
        }

        val data = input.copyOfRange(pos + 4, pos + len)
        lines.add(PktLine.Data(data))
        pos += len
    }

    return Pair(lines, pos)
}

// --- Smart protocol reference advertisement ---

/** A remote reference from the smart protocol handshake. */
data class RemoteRef(
    val oid: OID,
    val name: String
)

/** Server capabilities from the reference advertisement. */
class ServerCapabilities(
    val capabilities: List<String> = emptyList()
) {
    fun has(cap: String): Boolean =
        capabilities.any { it == cap || it.startsWith("$cap=") }

    fun get(cap: String): String? {
        val prefix = "$cap="
        return capabilities.firstOrNull { it.startsWith(prefix) }
            ?.removePrefix(prefix)
    }
}

/** Parse the reference advertisement from the smart protocol v1 response. */
fun parseRefAdvertisement(lines: List<PktLine>): Pair<List<RemoteRef>, ServerCapabilities> {
    val refs = mutableListOf<RemoteRef>()
    var capsList = listOf<String>()

    for ((i, line) in lines.withIndex()) {
        when (line) {
            is PktLine.Flush -> break
            is PktLine.Delim -> continue
            is PktLine.Data -> {
                var text = String(line.bytes, Charsets.UTF_8).trimEnd('\n')

                // Skip comment lines
                if (text.startsWith("#")) continue

                // First ref line may contain capabilities after NUL
                val nulPos = text.indexOf('\u0000')
                val refPart: String
                val capPart: String?

                if (nulPos >= 0) {
                    refPart = text.substring(0, nulPos)
                    capPart = text.substring(nulPos + 1)
                } else {
                    refPart = text
                    capPart = null
                }

                // Parse capabilities from first line
                if (i == 0 || capsList.isEmpty()) {
                    if (capPart != null) {
                        capsList = capPart.split(" ").filter { it.isNotEmpty() }
                    }
                }

                // Parse ref: "<oid> <refname>"
                if (refPart.length >= 41 && refPart[40] == ' ') {
                    val hex = refPart.substring(0, 40)
                    val name = refPart.substring(41)
                    try {
                        val oid = OID(hex)
                        refs.add(RemoteRef(oid, name))
                    } catch (_: Exception) {
                        // Skip invalid OIDs
                    }
                }
            }
        }
    }

    return Pair(refs, ServerCapabilities(capsList))
}

/** Build a want/have negotiation request for fetch. */
fun buildWantHave(wants: List<OID>, haves: List<OID>, caps: List<String>): ByteArray {
    var out = byteArrayOf()

    for ((i, want) in wants.withIndex()) {
        val line = if (i == 0 && caps.isNotEmpty()) {
            "want ${want.hex} ${caps.joinToString(" ")}\n"
        } else {
            "want ${want.hex}\n"
        }
        out += pktLineEncode(line.toByteArray(Charsets.UTF_8))
    }

    out += pktLineFlush()

    for (have in haves) {
        val line = "have ${have.hex}\n"
        out += pktLineEncode(line.toByteArray(Charsets.UTF_8))
    }

    out += pktLineEncode("done\n".toByteArray(Charsets.UTF_8))
    out += pktLineFlush()

    return out
}

/** Parse a URL into (scheme, host, path). */
fun parseGitUrl(url: String): Triple<String, String, String>? {
    // Handle SSH shorthand: user@host:path
    if ("://" !in url) {
        val colonIdx = url.indexOf(':')
        if (colonIdx >= 0 && '@' in url.substring(0, colonIdx)) {
            val host = url.substring(0, colonIdx)
            val path = url.substring(colonIdx + 1)
            return Triple("ssh", host, path)
        }
        return null
    }

    val schemeEnd = url.indexOf("://")
    if (schemeEnd < 0) return null
    val scheme = url.substring(0, schemeEnd)
    val rest = url.substring(schemeEnd + 3)

    val pathStart = rest.indexOf('/')
    val host: String
    val path: String
    if (pathStart >= 0) {
        host = rest.substring(0, pathStart)
        path = rest.substring(pathStart)
    } else {
        host = rest
        path = "/"
    }

    return Triple(scheme, host, path)
}
