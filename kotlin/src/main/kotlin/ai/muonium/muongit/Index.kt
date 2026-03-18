package ai.muonium.muongit

import java.io.ByteArrayOutputStream
import java.io.File

private val INDEX_SIGNATURE = byteArrayOf(0x44, 0x49, 0x52, 0x43) // "DIRC"
private const val INDEX_VERSION = 2
private const val ENTRY_FIXED_SIZE = 62 // 10*4 + 20 + 2

/** A single entry in the git index */
data class IndexEntry(
    val ctimeSecs: Int = 0,
    val ctimeNanos: Int = 0,
    val mtimeSecs: Int = 0,
    val mtimeNanos: Int = 0,
    val dev: Int = 0,
    val ino: Int = 0,
    val mode: Int,
    val uid: Int = 0,
    val gid: Int = 0,
    val fileSize: Int = 0,
    val oid: OID,
    val flags: Int = 0,
    val path: String,
)

/** The parsed git index */
data class Index(
    val version: Int = INDEX_VERSION,
    val entries: MutableList<IndexEntry> = mutableListOf(),
)

fun Index.add(entry: IndexEntry) {
    val idx = entries.indexOfFirst { it.path == entry.path }
    if (idx >= 0) {
        entries[idx] = entry
    } else {
        entries.add(entry)
        entries.sortBy { it.path }
    }
}

fun Index.remove(path: String): Boolean {
    val idx = entries.indexOfFirst { it.path == path }
    if (idx >= 0) {
        entries.removeAt(idx)
        return true
    }
    return false
}

fun Index.find(path: String): IndexEntry? =
    entries.firstOrNull { it.path == path }

// MARK: - Reading

/** Read and parse the git index file. */
fun readIndex(gitDir: File): Index {
    val indexFile = File(gitDir, "index")
    if (!indexFile.exists()) return Index()
    return parseIndex(indexFile.readBytes())
}

private fun readU32(data: ByteArray, offset: Int): Int =
    ((data[offset].toInt() and 0xFF) shl 24) or
    ((data[offset + 1].toInt() and 0xFF) shl 16) or
    ((data[offset + 2].toInt() and 0xFF) shl 8) or
    (data[offset + 3].toInt() and 0xFF)

private fun readU16(data: ByteArray, offset: Int): Int =
    ((data[offset].toInt() and 0xFF) shl 8) or
    (data[offset + 1].toInt() and 0xFF)

internal fun parseIndex(data: ByteArray): Index {
    if (data.size < 12) throw MuonGitException.InvalidObject("index too short")

    // Validate signature
    if (data[0] != 0x44.toByte() || data[1] != 0x49.toByte() ||
        data[2] != 0x52.toByte() || data[3] != 0x43.toByte()) {
        throw MuonGitException.InvalidObject("bad index signature")
    }

    val version = readU32(data, 4)
    if (version != 2) throw MuonGitException.InvalidObject("unsupported index version $version")

    val entryCount = readU32(data, 8)

    // Validate checksum
    if (data.size < 20) throw MuonGitException.InvalidObject("index too short for checksum")
    val content = data.copyOfRange(0, data.size - 20)
    val storedChecksum = data.copyOfRange(data.size - 20, data.size)
    val computed = SHA1.hash(content)
    if (!computed.contentEquals(storedChecksum)) {
        throw MuonGitException.InvalidObject("index checksum mismatch")
    }

    val entries = mutableListOf<IndexEntry>()
    var offset = 12

    for (i in 0 until entryCount) {
        if (offset + ENTRY_FIXED_SIZE > content.size) {
            throw MuonGitException.InvalidObject("index truncated")
        }

        val ctimeSecs = readU32(data, offset)
        val ctimeNanos = readU32(data, offset + 4)
        val mtimeSecs = readU32(data, offset + 8)
        val mtimeNanos = readU32(data, offset + 12)
        val dev = readU32(data, offset + 16)
        val ino = readU32(data, offset + 20)
        val mode = readU32(data, offset + 24)
        val uid = readU32(data, offset + 28)
        val gid = readU32(data, offset + 32)
        val fileSize = readU32(data, offset + 36)

        val oidBytes = data.copyOfRange(offset + 40, offset + 60)
        val oid = OID(oidBytes)
        val flags = readU16(data, offset + 60)

        // Read null-terminated path
        val pathStart = offset + ENTRY_FIXED_SIZE
        var pathEnd = pathStart
        while (pathEnd < content.size && data[pathEnd] != 0.toByte()) {
            pathEnd++
        }
        if (pathEnd >= content.size) {
            throw MuonGitException.InvalidObject("unterminated path in index")
        }

        val path = String(data, pathStart, pathEnd - pathStart, Charsets.UTF_8)

        // Compute padding to 8-byte alignment
        val entryLen = ENTRY_FIXED_SIZE + path.toByteArray(Charsets.UTF_8).size + 1
        val paddedLen = (entryLen + 7) and 7.inv()
        offset += paddedLen

        entries.add(IndexEntry(
            ctimeSecs = ctimeSecs, ctimeNanos = ctimeNanos,
            mtimeSecs = mtimeSecs, mtimeNanos = mtimeNanos,
            dev = dev, ino = ino, mode = mode, uid = uid, gid = gid,
            fileSize = fileSize, oid = oid, flags = flags, path = path,
        ))
    }

    return Index(version = version, entries = entries)
}

// MARK: - Writing

/** Write the index to the git directory. */
fun writeIndex(gitDir: File, index: Index) {
    val data = serializeIndex(index)
    File(gitDir, "index").writeBytes(data)
}

internal fun serializeIndex(index: Index): ByteArray {
    val buf = ByteArrayOutputStream()

    // Header
    buf.write(INDEX_SIGNATURE)
    buf.write(writeU32(index.version))

    // Sort entries by path
    val sorted = index.entries.sortedBy { it.path }
    buf.write(writeU32(sorted.size))

    for (entry in sorted) {
        buf.write(writeU32(entry.ctimeSecs))
        buf.write(writeU32(entry.ctimeNanos))
        buf.write(writeU32(entry.mtimeSecs))
        buf.write(writeU32(entry.mtimeNanos))
        buf.write(writeU32(entry.dev))
        buf.write(writeU32(entry.ino))
        buf.write(writeU32(entry.mode))
        buf.write(writeU32(entry.uid))
        buf.write(writeU32(entry.gid))
        buf.write(writeU32(entry.fileSize))
        buf.write(entry.oid.raw)

        // Flags: lower 12 bits = min(path_len, 0xFFF), upper bits from entry
        val pathBytes = entry.path.toByteArray(Charsets.UTF_8)
        val nameLen = minOf(pathBytes.size, 0xFFF)
        val flags = (entry.flags and 0xF000.toInt()) or nameLen
        buf.write(writeU16(flags))

        // Path + null padding to 8-byte alignment
        buf.write(pathBytes)
        val entryLen = ENTRY_FIXED_SIZE + pathBytes.size + 1
        val paddedLen = (entryLen + 7) and 7.inv()
        val padCount = paddedLen - ENTRY_FIXED_SIZE - pathBytes.size
        buf.write(ByteArray(padCount))
    }

    // Checksum
    val content = buf.toByteArray()
    val checksum = SHA1.hash(content)
    buf.write(checksum)

    return buf.toByteArray()
}

private fun writeU32(value: Int): ByteArray = byteArrayOf(
    ((value shr 24) and 0xFF).toByte(),
    ((value shr 16) and 0xFF).toByte(),
    ((value shr 8) and 0xFF).toByte(),
    (value and 0xFF).toByte(),
)

private fun writeU16(value: Int): ByteArray = byteArrayOf(
    ((value shr 8) and 0xFF).toByte(),
    (value and 0xFF).toByte(),
)
